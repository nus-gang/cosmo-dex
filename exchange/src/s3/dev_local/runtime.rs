use super::{
    Error, Hook, Result, Validated, fault,
    store::{Store, frame},
    with_hook,
};
use crate::s3::{
    evidence::{self, Objects},
    journal::{Commit, canonical, sha256},
    record::Prepared,
    schema,
    sequencer::Candidate,
    snapshot::{Observation, Snapshot},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, MutexGuard, RwLock},
};
/// All input is from the trusted, authenticated D adapter. Session identity must
/// come from authentication, never from a request body. No network IO here.
pub enum Command {
    Signed {
        kind: String,
        raw: Vec<u8>,
        signature: Vec<u8>,
        session_owner: String,
    },
    Local {
        kind: String,
        raw: Vec<u8>,
        session_owner: String,
    },
    Snapshot(Vec<u8>),
    Seal(String),
    Attempt(Value),
    Resolve(Value),
    Receipt(Value),
    RejectFinal,
    Apply,
}
#[derive(Clone, Debug)]
pub struct View {
    pub gate: String,
    pub commit: Commit,
    pub state: Value,
    pub receipts: BTreeMap<u64, Value>,
}
/// Private trusted-runtime transport, never a public REST response or an effect
/// permit. Raw bytes and decoded snapshot have been checked against this store.
#[derive(Clone, Debug)]
pub struct RecoveryObservation {
    pub snapshot: Snapshot,
    pub raw: Vec<u8>,
}
/// Inclusive height page plus applied/latest anchors from exactly one commit.
#[derive(Clone, Debug)]
pub struct RecoveryHistory {
    pub commit: Commit,
    pub first_height: u64,
    pub applied: RecoveryObservation,
    pub latest: RecoveryObservation,
    pub observations: Vec<RecoveryObservation>,
    pub next_height: Option<u64>,
}
/// One committed attempt and only its reachable evidence (including original
/// TxRaw and terminal proof). No callback, path input, or broadcast authority.
#[derive(Clone, Debug)]
pub struct RecoveryAttempt {
    pub commit: Commit,
    pub attempt: Value,
    pub evidence: Objects,
}
/// Original committed failure evidence for one batch. Private trusted-runtime
/// transport only; the bytes come from the store, never a new failure selection.
#[derive(Clone, Debug)]
pub struct RecoveryFailure {
    pub commit: Commit,
    pub resolution_evidence_ref: Value,
    pub resolution_evidence: Value,
    pub raw: Vec<u8>,
    pub evidence: Objects,
}
/// Apply is tested on a private clone using the same implementation as execute.
/// Held is a reconciliation condition, never authorization to discard a fill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyReadiness {
    NoPendingObservation,
    Ready,
    Held { reason: &'static str },
}
/// Trusted runtime transport only. Valid for this commit, observation and time;
/// no receipt, reservation, callback or permission to bypass execute validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileReadiness {
    pub commit: Commit,
    pub seal: super::SealReadiness,
    pub apply: ApplyReadiness,
    pub applied_height: u64,
    pub latest_height: u64,
}
pub const RECOVERY_HISTORY_PAGE_MAX: usize = 64;
/// Internal full state/result/book/FIFO/cursor projection in one immutable Arc.
/// Never expose all owners or signed evidence directly through public REST.
#[derive(Clone)]
pub struct ReadView(Arc<RwLock<Arc<View>>>);
impl ReadView {
    pub fn get(&self) -> Result<Arc<View>> {
        Ok(self
            .0
            .read()
            .map_err(|_| Error::Recovery("PUBLISHER_POISONED"))?
            .clone())
    }
}
struct Writer {
    store: Store,
    candidate: Candidate,
    config: Validated,
    hook: Option<Hook>,
    closed: bool,
}
impl Writer {
    fn check_store(&self) -> Result<Vec<u8>> {
        let bootstrap = self.store.check()?;
        // Removing a root and its descriptor together can pass directory
        // inventory checks. Pin these private roots at every read/effect/write
        // boundary too, so a missing original cannot authorize a later CLOSE.
        for id in self.candidate.failure_evidence.keys() {
            load_failure(&self.store, &self.candidate, &self.config, id)?;
        }
        Ok(bootstrap)
    }
    fn ensure_open(&self) -> Result<()> {
        // IO failures close this writer; semantic recovery is part of the
        // committed candidate and survives open/replay. Check both under the
        // writer lock before any mutation or effect. CATCHING_UP still permits
        // the existing reconciliation commands.
        if self.closed || self.candidate.mode() == "RECOVERY_REQUIRED" {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        Ok(())
    }
}
pub struct Engine {
    writer: Mutex<Writer>,
    reader: ReadView,
}
fn receipt(c: &Validated, result: Value) -> Value {
    json!({"envelope_version":"s3-dev-local/1","profile_id":"s3-dev-local-v1","context":c.context(),"development_receipt":"LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE","durable_ack":false,"storage_assurance":"UNPROVEN_HOST_SPACE","command_result":result})
}
fn ledger_entry(c: &Validated, result: Value, commit: &Commit) -> Value {
    json!({"receipt":receipt(c,result),"command_seq":commit.command_seq.to_string(),"record_hash":commit.record_hash,"end_offset":commit.end_offset.to_string()})
}
fn load_failure(
    store: &Store,
    candidate: &Candidate,
    config: &Validated,
    batch_id: &str,
) -> Result<Option<RecoveryFailure>> {
    let Some(original) = candidate.failure_evidence.get(batch_id) else {
        return Ok(None);
    };
    let batch = candidate
        .batches()
        .iter()
        .find(|b| b["batch"]["batch_id"] == batch_id)
        .ok_or(Error::Recovery("RECOVERY_FAILURE_BATCH"))?;
    // Derive only a content-addressed selector from the committed candidate.
    // Missing stored bytes are an error; do not synthesize/backfill the object.
    let r = evidence::reference(&canonical(original)?, evidence::TYPED)?;
    let root = json!({"resolution_evidence_ref":r});
    let refs = candidate.evidence_set()?.graph(&root)?;
    // Existing schema: one typed root, <=3 inline attempts, each <=1 TX,
    // 2 inclusion RPC and 8 pairs of absence RPC. No schema/cap changes.
    if refs.len() > 58 {
        return Err(Error::Recovery("RECOVERY_EVIDENCE_BOUND"));
    }
    let refs = json!(refs);
    let objects = store.load(&refs)?;
    objects.verify_exact_refs(&root, &refs)?;
    let resolution = objects.typed(&r, "ResolutionEvidence")?;
    if resolution != *original
        || resolution["context"] != *config.context()
        || resolution["batch"] != batch["batch"]
    {
        return Err(Error::Recovery("RECOVERY_FAILURE_MISMATCH"));
    }
    Ok(Some(RecoveryFailure {
        commit: store.commit.clone(),
        raw: objects.resolve(&r, evidence::TYPED)?.to_vec(),
        resolution_evidence_ref: r,
        resolution_evidence: resolution,
        evidence: objects,
    }))
}
impl Engine {
    fn recovery_writer(&self, expected: &Commit) -> Result<(MutexGuard<'_, Writer>, Vec<u8>)> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Recovery("WRITER_POISONED"))?;
        w.ensure_open()?;
        let bootstrap = match w.check_store() {
            Ok(raw) => raw,
            Err(e) => {
                self.close(&mut w)?;
                return Err(super::storage_error(e));
            }
        };
        if w.candidate.sequence() != w.store.commit.command_seq
            || self.reader.get()?.commit != w.store.commit
        {
            self.close(&mut w)?;
            return Err(Error::Recovery("RECOVERY_COMMIT_MISMATCH"));
        }
        if *expected != w.store.commit {
            return Err(Error::Invalid("STALE_COMMIT"));
        }
        Ok((w, bootstrap))
    }
    fn finish_recovery_read<T>(&self, w: &mut Writer, result: Result<T>) -> Result<T> {
        // Recheck identity/marker after the bounded read, still under the writer
        // lock. An integrity failure closes all later mutations and effects.
        match result.and_then(|v| {
            w.check_store()?;
            Ok(v)
        }) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.close(w)?;
                Err(super::storage_error(e))
            }
        }
    }
    /// Select Seal purpose/wait and assess Apply under the same store/writer
    /// lock. The caller supplies the latest observation, not the applied one.
    /// Prefer Ready Apply, otherwise Ready Seal, otherwise reconcile ActiveBatch
    /// or await a fresh observation. Never retry with another purpose on error.
    pub fn trusted_reconcile_readiness(
        &self,
        expected: &Commit,
        observation: &Observation,
        now: u64,
    ) -> Result<ReconcileReadiness> {
        let (mut w, _) = self.recovery_writer(expected)?;
        // Inventory alone cannot detect both halves of an object disappearing,
        // or an in-place WAL edit of unchanged length. Validate this committed
        // prefix and every referenced object with the store's existing parser.
        let integrity = w.store.verify_committed();
        self.finish_recovery_read(&mut w, integrity)?;
        w.candidate.latest().freshness(observation, now)?;
        let apply = if w.candidate.observations.is_empty() {
            ApplyReadiness::NoPendingObservation
        } else {
            match w.candidate.apply() {
                Ok(next) => {
                    if !next.capacity()?.admissible() {
                        return Err(Error::Invalid("STORAGE_CAPACITY"));
                    }
                    ApplyReadiness::Ready
                }
                Err(reason @ ("UNSETTLED_HOLD" | "ATTEMPT_UNRESOLVED")) => {
                    ApplyReadiness::Held { reason }
                }
                Err(reason) => return Err(Error::Invalid(reason)),
            }
        };
        let seal = w.candidate.seal_readiness()?;
        if let super::SealReadiness::Ready(purpose) = &seal {
            // Validate exactly the selected purpose, including wire/proof/cap
            // checks. Discard the candidate; no identity is published or stored.
            let next = w.candidate.seal_batch(purpose.as_str(), observation, now)?;
            if !next.capacity()?.admissible() {
                return Err(Error::Invalid("STORAGE_CAPACITY"));
            }
        }
        let result = ReconcileReadiness {
            commit: w.store.commit.clone(),
            seal,
            apply,
            applied_height: w.candidate.snapshot().height(),
            latest_height: w.candidate.latest().height(),
        };
        let result = w.store.verify_committed().map(|()| result);
        self.finish_recovery_read(&mut w, result)
    }
    /// Trusted runtime only. Pin all pages and attempt reads to reader().commit;
    /// STALE_COMMIT means discard the accumulated recovery view and start again.
    /// None starts at bootstrap; the page includes from_height, at most 64 rows.
    /// CATCHING_UP is readable; RECOVERY_REQUIRED and storage errors stay closed.
    pub fn trusted_recovery_history(
        &self,
        expected: &Commit,
        from_height: Option<u64>,
        limit: usize,
    ) -> Result<RecoveryHistory> {
        let (mut w, bootstrap) = self.recovery_writer(expected)?;
        if !(1..=RECOVERY_HISTORY_PAGE_MAX).contains(&limit) {
            return Err(Error::Invalid("RECOVERY_PAGE_LIMIT"));
        }
        let first = w
            .candidate
            .history
            .first()
            .ok_or(Error::Recovery("RECOVERY_HISTORY_MISSING"))?
            .height();
        let from = from_height.unwrap_or(first);
        let Ok(start) = w
            .candidate
            .history
            .binary_search_by_key(&from, Snapshot::height)
        else {
            return Err(Error::Invalid("RECOVERY_HEIGHT_RANGE"));
        };
        let result = (|| {
            let read = |s: &Snapshot| -> Result<RecoveryObservation> {
                let raw = if s.height() == first {
                    bootstrap.clone()
                } else {
                    let r = evidence::reference(&canonical(s.value())?, evidence::TYPED)?;
                    w.store.read(&r)?
                };
                let snapshot = w.candidate.snapshot().decode_related(&raw)?;
                if snapshot != *s {
                    return Err(Error::Recovery("RECOVERY_SNAPSHOT_MISMATCH"));
                }
                Ok(RecoveryObservation { snapshot, raw })
            };
            let end = start.saturating_add(limit).min(w.candidate.history.len());
            let mut observations = Vec::with_capacity(end - start);
            let mut previous = if start > 0 {
                Some(read(&w.candidate.history[start - 1])?.snapshot)
            } else {
                None
            };
            for s in &w.candidate.history[start..end] {
                let observation = read(s)?;
                if let Some(prev) = &previous {
                    if !prev.advance(&observation.snapshot)? {
                        return Err(Error::Recovery("RECOVERY_HISTORY_CONTINUITY"));
                    }
                }
                previous = Some(observation.snapshot.clone());
                observations.push(observation);
            }
            Ok(RecoveryHistory {
                commit: w.store.commit.clone(),
                first_height: first,
                applied: read(w.candidate.snapshot())?,
                latest: read(w.candidate.latest())?,
                observations,
                next_height: w.candidate.history.get(end).map(Snapshot::height),
            })
        })();
        self.finish_recovery_read(&mut w, result)
    }
    /// Reads PREPARED/UNKNOWN/terminal attempts without entering the effect API.
    /// Selection is a committed tx hash, never a caller-supplied path or ref.
    pub fn trusted_recovery_attempt(
        &self,
        expected: &Commit,
        tx_hash: &str,
    ) -> Result<Option<RecoveryAttempt>> {
        let (mut w, _) = self.recovery_writer(expected)?;
        schema::validate("Hash", &json!(tx_hash))?;
        let Some(attempt) = w
            .candidate
            .attempts()
            .iter()
            .find(|a| a["tx_hash"] == tx_hash)
            .cloned()
        else {
            return Ok(None);
        };
        self.read_recovery_attempt(&mut w, attempt).map(Some)
    }
    /// Cold-start discovery without a sidecar or remembered TX hash. The index
    /// is the position in this commit's View.state["attempt_refs"]. Each call
    /// returns at most one attempt and its bounded graph; out of range is None.
    pub fn trusted_recovery_attempt_at(
        &self,
        expected: &Commit,
        index: usize,
    ) -> Result<Option<RecoveryAttempt>> {
        let (mut w, _) = self.recovery_writer(expected)?;
        let Some(attempt) = w.candidate.attempts().get(index).cloned() else {
            return Ok(None);
        };
        self.read_recovery_attempt(&mut w, attempt).map(Some)
    }
    /// Select only by a batch ID from the same committed View. None means no
    /// stored final rejection for that batch, never permission to CLOSE/VOID.
    /// Reads also work after correction; the original observation is preserved.
    pub fn trusted_recovery_failure(
        &self,
        expected: &Commit,
        batch_id: &str,
    ) -> Result<Option<RecoveryFailure>> {
        let (mut w, _) = self.recovery_writer(expected)?;
        schema::validate("Hash", &json!(batch_id))?;
        let result = load_failure(&w.store, &w.candidate, &w.config, batch_id);
        self.finish_recovery_read(&mut w, result)
    }
    fn read_recovery_attempt(&self, w: &mut Writer, attempt: Value) -> Result<RecoveryAttempt> {
        let result = (|| {
            let r = evidence::reference(&canonical(&attempt)?, evidence::TYPED)?;
            let root = json!({"attempt_refs":[r]});
            let refs = w.candidate.evidence_set()?.graph(&root)?;
            // Existing Attempt schema: typed attempt + TxRaw + 2 inclusion
            // responses + at most 8 pairs of absence responses. No cap change.
            if refs.len() > 20 {
                return Err(Error::Recovery("RECOVERY_EVIDENCE_BOUND"));
            }
            let refs = json!(refs);
            let objects = w.store.load(&refs)?;
            objects.verify_exact_refs(&root, &refs)?;
            if objects.typed(&r, "Attempt")? != attempt
                || attempt["context"] != *w.config.context()
                || sha256(objects.resolve(&attempt["raw_tx_ref"], evidence::TX)?)
                    != attempt["tx_hash"]
            {
                return Err(Error::Recovery("RECOVERY_ATTEMPT_MISMATCH"));
            }
            Ok(RecoveryAttempt {
                commit: w.store.commit.clone(),
                attempt,
                evidence: objects,
            })
        })();
        self.finish_recovery_read(w, result)
    }
    #[cfg(feature = "fault-injection")]
    pub fn create_with_fault_hook(
        path: &Path,
        c: Validated,
        bootstrap: &[u8],
        hook: Arc<dyn Fn(&str) -> Result<()> + Send + Sync>,
    ) -> Result<Self> {
        with_hook(&Some(hook), || Self::create(path, c, bootstrap))
    }
    pub fn create(path: &Path, c: Validated, bootstrap: &[u8]) -> Result<Self> {
        let candidate = Candidate::new(c.bootstrap(bootstrap)?)?;
        if !candidate.capacity()?.admissible() {
            return Err(Error::Invalid("STORAGE_CAPACITY"));
        }
        let store = Store::create(path, c.clone(), bootstrap)?;
        Self::assembled(store, c, candidate, BTreeMap::new())
    }
    pub fn open(path: &Path, c: Validated) -> Result<Self> {
        Self::open_inner(path, c).map_err(super::storage_error)
    }
    fn open_inner(path: &Path, c: Validated) -> Result<Self> {
        let (store, raw, records) = Store::open(path, c.clone())?;
        let mut candidate = Candidate::new(c.bootstrap(&raw)?)?;
        let mut commit = Commit::zero();
        let mut receipts = BTreeMap::new();
        for r in records {
            let objects = store.load(&r["evidence_refs"])?;
            let (next, prepared) = Prepared::replay(&candidate, &r, &objects, &commit)
                .map_err(|_| Error::Recovery("SEMANTIC_REPLAY"))?;
            if !next.capacity()?.admissible() {
                return Err(Error::Recovery("STORAGE_CAPACITY"));
            }
            let b = frame(&canonical(&r)?)?;
            commit = Commit {
                command_seq: next.sequence(),
                record_hash: sha256(&b),
                end_offset: commit
                    .end_offset
                    .checked_add(b.len() as u64)
                    .ok_or(Error::Recovery("OFFSET_OVERFLOW"))?,
            };
            receipts.insert(
                commit.command_seq,
                ledger_entry(&c, prepared.result, &commit),
            );
            candidate = next;
        }
        if commit != store.commit {
            return Err(Error::Recovery("REPLAY_COMMIT"));
        }
        // A replayed selection is not the original byte source. Older homes
        // missing a failure root stay closed, without automatic migration.
        for id in candidate.failure_evidence.keys() {
            load_failure(&store, &candidate, &c, id)?;
        }
        Self::assembled(store, c, candidate, receipts)
    }
    fn assembled(
        store: Store,
        config: Validated,
        candidate: Candidate,
        receipts: BTreeMap<u64, Value>,
    ) -> Result<Self> {
        let view = View {
            gate: candidate.mode().into(),
            commit: store.commit.clone(),
            state: candidate.full_state()?,
            receipts,
        };
        Ok(Self {
            writer: Mutex::new(Writer {
                store,
                candidate,
                config,
                hook: None,
                closed: false,
            }),
            reader: ReadView(Arc::new(RwLock::new(Arc::new(view)))),
        })
    }
    pub fn reader(&self) -> ReadView {
        self.reader.clone()
    }
    #[cfg(feature = "fault-injection")]
    pub fn set_fault_hook(
        &self,
        hook: Option<Arc<dyn Fn(&str) -> Result<()> + Send + Sync>>,
    ) -> Result<()> {
        self.writer
            .lock()
            .map_err(|_| Error::Recovery("WRITER_POISONED"))?
            .hook = hook;
        Ok(())
    }
    fn close(&self, w: &mut Writer) -> Result<()> {
        w.closed = true;
        let old = self.reader.get()?;
        *self
            .reader
            .0
            .write()
            .map_err(|_| Error::Recovery("PUBLISHER_POISONED"))? = Arc::new(View {
            gate: "RECOVERY_REQUIRED".into(),
            ..(*old).clone()
        });
        Ok(())
    }
    pub fn execute(
        &self,
        command: Command,
        raw_evidence: &[(Vec<u8>, String)],
        observation: &Observation,
        now: u64,
    ) -> Result<Option<Value>> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Recovery("WRITER_POISONED"))?;
        w.ensure_open()?;
        if let Err(e) = w.check_store() {
            self.close(&mut w)?;
            return Err(super::storage_error(e));
        }
        let mut input = w.candidate.clone();
        // Bound and validate all incoming exact raw bytes before copying/storing.
        for (raw, media) in raw_evidence {
            input.provide_evidence(raw, media)?;
        }
        let (after, kind, duplicate) = match command {
            Command::Signed {
                kind,
                raw,
                signature,
                session_owner,
            } => {
                let (n, r, d) =
                    input.submit(&kind, &raw, &signature, &session_owner, observation, now)?;
                (n, kind, if d { Some(r.seq) } else { None })
            }
            Command::Local {
                kind,
                raw,
                session_owner,
            } => {
                let (n, r, d) =
                    input.local_action(&kind, &raw, &session_owner, observation, now)?;
                (n, kind, if d { Some(r.seq) } else { None })
            }
            Command::Snapshot(raw) => (
                input.observe(input.snapshot().decode_related(&raw)?)?,
                "SNAPSHOT".into(),
                None,
            ),
            Command::Seal(p) => (
                input.seal_batch(&p, observation, now)?,
                "SEAL_BATCH".into(),
                None,
            ),
            Command::Attempt(v) => (input.prepare_attempt(v)?, "ATTEMPT".into(), None),
            Command::Resolve(v) => (input.resolve_attempt(v)?, "RESOLVE_ATTEMPT".into(), None),
            Command::Receipt(v) => {
                let k = if v["disposition"] == "COMMITTED" {
                    "RESOLVE_ATTEMPT"
                } else {
                    "VOID_BATCH"
                };
                (input.record_receipt(v)?, k.into(), None)
            }
            Command::RejectFinal => (
                input.reject_final(input.rejection_evidence()?)?,
                "VOID_BATCH".into(),
                None,
            ),
            Command::Apply => {
                let n = input.apply()?;
                let k = if n.full_state()?["corrections"] != input.full_state()?["corrections"] {
                    "CORRECTION"
                } else {
                    "SETTLEMENT_APPLY"
                };
                (n, k.into(), None)
            }
        };
        if let Some(seq) = duplicate {
            return Ok(Some(
                self.reader
                    .get()?
                    .receipts
                    .get(&seq)
                    .ok_or(Error::Recovery("RESULT_MISSING"))?["receipt"]
                    .clone(),
            ));
        }
        if after.sequence() == input.sequence() {
            return Ok(None);
        }
        if !after.capacity()?.admissible() {
            return Err(Error::Invalid("STORAGE_CAPACITY"));
        }
        let p = Prepared::prepare(&input, &after, &kind, observation, now, &w.store.commit)?;
        let full = after.evidence_set()?;
        let mut objects = Objects::default();
        for r in p.record["evidence_refs"].as_array().unwrap() {
            objects.insert(
                full.resolve(r, r["media_type"].as_str().unwrap())?,
                r["media_type"].as_str().unwrap(),
            )?;
        }
        // EngineState/JournalRecord deliberately do not expose failure roots
        // before a VOID receipt. Persist new roots and their closure privately
        // in the same existing object/WAL transaction, before any response.
        let new_failures: Vec<_> = after
            .failure_evidence
            .keys()
            .filter(|id| !w.candidate.failure_evidence.contains_key(*id))
            .cloned()
            .collect();
        for id in &new_failures {
            let r = evidence::reference(&canonical(&after.failure_evidence[id])?, evidence::TYPED)?;
            for r in full.graph(&json!({"resolution_evidence_ref":r}))? {
                let media = r["media_type"].as_str().unwrap();
                objects.insert(full.resolve(&r, media)?, media)?;
            }
        }
        let hook = w.hook.clone();
        let outcome = with_hook(&hook, || {
            w.store.begin(&objects)?;
            let stored = w.store.load(&p.record["evidence_refs"])?;
            let (replayed, verified) =
                Prepared::replay(&w.candidate, &p.record, &stored, &w.store.commit)
                    .map_err(|_| Error::Recovery("SEMANTIC_PREPARE"))?;
            if replayed.full_state()? != after.full_state()? || verified.result != p.result {
                return Err(Error::Recovery("SEMANTIC_MISMATCH"));
            }
            for id in &new_failures {
                load_failure(&w.store, &replayed, &w.config, id)?;
            }
            fault("candidate_verified")?;
            let commit = w.store.append(&p.record)?;
            let mut next = (*self.reader.get()?).clone();
            next.commit = commit.clone();
            next.state = replayed.full_state()?;
            next.gate = replayed.mode().into();
            next.receipts.insert(
                commit.command_seq,
                ledger_entry(&w.config, p.result.clone(), &commit),
            );
            fault("before_publish")?;
            // The only publication: complete state/results/book/FIFO/cursor together.
            *self
                .reader
                .0
                .write()
                .map_err(|_| Error::Recovery("PUBLISHER_POISONED"))? = Arc::new(next);
            w.candidate = replayed;
            fault("before_response")?;
            Ok(receipt(&w.config, p.result))
        });
        match outcome {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                self.close(&mut w)?;
                Err(super::storage_error(e))
            }
        }
    }
    /// Read-only authenticated signed retry lookup; never creates a new binding.
    /// Historical results remain queryable while the mutation/effect gate is closed.
    pub fn query_signed(
        &self,
        kind: &str,
        raw: &[u8],
        sig: &[u8],
        session_owner: &str,
        o: &Observation,
        now: u64,
    ) -> Result<Option<Value>> {
        let w = self
            .writer
            .lock()
            .map_err(|_| Error::Recovery("WRITER_POISONED"))?;
        let (_, result, duplicate) = w.candidate.submit(kind, raw, sig, session_owner, o, now)?;
        if !duplicate {
            return Ok(None);
        }
        Ok(Some(
            self.reader
                .get()?
                .receipts
                .get(&result.seq)
                .ok_or(Error::Recovery("RESULT_MISSING"))?["receipt"]
                .clone(),
        ))
    }
    pub fn reconcile_receipt_ledger(&self, entries: &[Value]) -> Result<()> {
        let view = self.reader.get()?;
        for entry in entries {
            let seq = schema::num(&entry["command_seq"])?;
            if view.receipts.get(&seq) != Some(entry) {
                return Err(Error::Recovery("CLIENT_RECEIPT_MISMATCH"));
            }
        }
        Ok(())
    }
    /// Execute D's bounded effect callback while holding the single writer gate.
    /// Receipt, raw TX and current store identity are checked before invocation.
    pub fn with_committed_attempt<T>(
        &self,
        tx_hash: &str,
        effect: impl FnOnce(&Value, &[u8]) -> T,
    ) -> Result<Option<T>> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Recovery("WRITER_POISONED"))?;
        w.ensure_open()?;
        if let Err(e) = w.check_store() {
            self.close(&mut w)?;
            return Err(super::storage_error(e));
        }
        let Some(attempt) = w
            .candidate
            .attempts()
            .iter()
            .find(|a| a["tx_hash"] == tx_hash)
            .cloned()
        else {
            return Ok(None);
        };
        if !["PREPARED", "SUBMISSION_UNKNOWN"].contains(&attempt["state"].as_str().unwrap_or("")) {
            return Err(Error::Invalid("ATTEMPT_TERMINAL"));
        }
        let raw = match w.store.read(&attempt["raw_tx_ref"]) {
            Ok(raw) => raw,
            Err(e) => {
                self.close(&mut w)?;
                return Err(super::storage_error(e));
            }
        };
        // The callback must not reenter Engine. D persists its own broadcast
        // intent before entering, reports UNKNOWN on ambiguous network results,
        // and reconciles through a later command. No automatic retry here.
        Ok(Some(effect(&attempt, &raw)))
    }
    /// Read-only lookup. Use with_committed_attempt to serialize a broadcast.
    pub fn committed_attempt(&self, tx_hash: &str) -> Result<Option<Value>> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Recovery("WRITER_POISONED"))?;
        w.ensure_open()?;
        if let Err(e) = w.check_store() {
            self.close(&mut w)?;
            return Err(super::storage_error(e));
        }
        Ok(w.candidate
            .attempts()
            .iter()
            .rev()
            .find(|a| a["tx_hash"] == tx_hash)
            .cloned())
    }
}

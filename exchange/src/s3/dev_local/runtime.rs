use super::{
    Error, Hook, Result, Validated, fault,
    store::{Store, frame},
    with_hook,
};
use crate::s3::{
    evidence::Objects,
    journal::{Commit, canonical, sha256},
    record::Prepared,
    schema,
    sequencer::Candidate,
    snapshot::Observation,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, RwLock},
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
impl Engine {
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
        if w.closed {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        if let Err(e) = w.store.check() {
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
        if w.closed || w.candidate.mode() == "RECOVERY_REQUIRED" {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        if let Err(e) = w.store.check() {
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
        if w.closed {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        if let Err(e) = w.store.check() {
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

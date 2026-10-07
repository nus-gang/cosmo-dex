//! Trusted submission wiring. Never exposed through REST. C owns all transitions.
#[path = "collect.rs"]
mod collect;
#[path = "recovery.rs"]
mod recovery;
use collect::{Account, ChainRead};
use nus_exchange_contract::s3::{
    dev_local::{ApplyReadiness, SealPurpose, SealReadiness, Command, Engine, Error, Result},
    evidence::{Objects, TYPED, reference},
    journal::canonical,
    schema,
    settlement_local::{LoopbackRpc, Worker, chain::OperatorSigner},
    snapshot::{Observation, Snapshot},
};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Prepared {
    pub tx_hash: String,
    /// Audit bytes only; not a chain receipt or a C terminal proof.
    pub account_rpc: Vec<u8>,
}
/// Scan progress is scheduling information, never a terminal/absence proof.
#[derive(Debug, PartialEq, Eq)]
pub enum InclusionProgress {
    Waiting,
    Missing(u64),
    Included(u64),
    WindowScanned,
}
/// A single unresolved-attempt tick. These are control-flow results, not receipts.
#[derive(Debug, PartialEq, Eq)]
pub enum PendingProgress { Broadcast, Scan(InclusionProgress), AbsenceProven }
#[derive(Debug, PartialEq, Eq)]
enum PendingAction { Broadcast, Scan, Absence }
/// Private scheduling result. Never a receipt or a broadcast permission.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BatchProgress {
    Pending(String),
    CommittedReceipt(usize),
    VoidReceipt(usize),
    RejectFinal,
    PrepareSettle { batch: String, attempt: u64 },
    PrepareClose { batch: String, attempt: u64 },
}
/// One outer reconciliation action. Idle is not a Seal permission or receipt.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ReconcileProgress { Active(BatchProgress), Apply, Seal(SealPurpose), Idle }
pub struct SubmitLane {
    engine: Arc<Engine>,
    worker: Worker,
    inclusion_next: Option<(String, u64)>,
    closed: bool,
}
fn now() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("CLOCK"))?
            .as_millis(),
    )
    .map_err(|_| Error::Invalid("CLOCK"))
}
impl SubmitLane {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            worker: Worker::new(engine.clone()),
            engine,
            inclusion_next: None,
            closed: false,
        }
    }
    fn bound(&self, s: &Snapshot, o: &Observation, now: u64) -> Result<()> {
        let v = self.engine.reader().get()?;
        if v.gate == "RECOVERY_REQUIRED" {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        let latest = &v.state["latest_observation_ref"];
        let matches = if latest.is_null() {
            v.state["chain_snapshot"] == *s.value()
        } else {
            *latest == reference(&canonical(s.value())?, TYPED)?
        };
        if !matches || v.state["context"] != *s.context() {
            return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
        }
        s.freshness(o, now)?;
        Ok(())
    }
    /// Read durable state on every tick; RAM scan completion never authorizes
    /// absence. UNKNOWN is never automatically rebroadcast or re-signed here.
    fn pending_action(&self, s: &Snapshot, hash: &str, o: &Observation, at: u64)
        -> Result<PendingAction> {
        self.bound(s, o, at)?;
        let view = self.engine.reader().get()?;
        let saved = self.engine.trusted_recovery_attempt(&view.commit, hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        let a = &saved.attempt;
        if !matches!(a["state"].as_str(), Some("PREPARED" | "SUBMISSION_UNKNOWN")) {
            return Err(Error::Invalid("ATTEMPT_TERMINAL"));
        }
        let timeout = schema::num(&a["timeout_height"])?;
        if a["state"] == "PREPARED" && a["broadcast_count"] == "0"
            && s.height() < timeout && a["operator"] == s.value()["operator"]
            && a["operator_epoch"] == s.value()["operator_epoch"] {
            return Ok(PendingAction::Broadcast);
        }
        if matches!(&self.inclusion_next, Some((old, next)) if old == hash && *next > timeout)
            && s.height() > timeout {
            return Ok(PendingAction::Absence);
        }
        Ok(PendingAction::Scan)
    }
    /// At most one existing lane action. Absence still fetches and validates the
    /// complete timeout proof; a scan miss alone never changes durable state.
    pub fn pending_tick(&mut self, chain: &ChainRead, rpc: &LoopbackRpc,
        s: &Snapshot, hash: &str, o: &Observation) -> Result<PendingProgress> {
        self.pending_tick_with(s, hash, o, now, |lane, action| match action {
            PendingAction::Broadcast => {
                lane.broadcast_existing(hash, rpc, s, o)?;
                Ok(PendingProgress::Broadcast)
            }
            PendingAction::Scan => Ok(PendingProgress::Scan(lane.scan_inclusion(chain, s, hash, o)?)),
            PendingAction::Absence => {
                lane.resolve_absence(chain, s, hash, o)?;
                Ok(PendingProgress::AbsenceProven)
            }
        })
    }
    fn pending_tick_with(&mut self, s: &Snapshot, hash: &str, o: &Observation,
        clock: impl FnOnce() -> Result<u64>,
        effect: impl FnOnce(&mut Self, PendingAction) -> Result<PendingProgress>) -> Result<PendingProgress> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        let action = self.pending_action(s, hash, o, clock()?)?;
        // Existing action methods manage their own closed flag. Catch unwind so
        // an unexpected panic between dispatch and the action also closes us.
        self.closed = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| effect(self, action)));
        match result {
            Ok(Ok(progress)) => Ok(progress),
            Ok(Err(e)) => { self.closed = true; Err(e) }
            Err(panic) => { self.closed = true; std::panic::resume_unwind(panic) }
        }
    }
    /// Select only the unresolved batch from a single durable C revision.
    /// No active batch means the outer driver must decide Seal/Apply separately.
    fn active_action(&self, s: &Snapshot, o: &Observation, at: u64)
        -> Result<(nus_exchange_contract::s3::journal::Commit, Option<BatchProgress>)> {
        self.bound(s, o, at)?;
        let view = self.engine.reader().get()?;
        let batches = view.state["batches"].as_array().ok_or("BATCH_STATE")?;
        let receipts = view.state["resolution_receipts"].as_array().ok_or("BATCH_STATE")?;
        let active: Vec<_> = batches.iter().filter(|b| {
            !matches!(b["state"].as_str(), Some("COMMITTED" | "CORRECTED"))
                && !receipts.iter().any(|r| r["batch"] == b["batch"])
        }).collect();
        if active.len() > 1 { return Err(Error::Recovery("MULTIPLE_ACTIVE_BATCHES")); }
        let Some(b) = active.first() else { return Ok((view.commit.clone(), None)); };
        let state = b["state"].as_str().ok_or("BATCH_STATE")?;
        if !matches!(state, "SEALED" | "SUBMISSION_UNKNOWN" | "REJECTED_FINAL" | "CLOSING") {
            return Err(Error::Recovery("BATCH_STATE"));
        }
        let hashes = b["attempt_hashes"].as_array().ok_or("BATCH_STATE")?;
        if hashes.len() > 5 { return Err(Error::Recovery("ATTEMPT_BUDGET")); }
        let refs = view.state["attempt_refs"].as_array().ok_or("BATCH_STATE")?;
        let mut pending = None;
        let mut success = None;
        let (mut settles, mut closes, mut failed_settle) = (0u64, 0u64, false);
        for hash in hashes {
            let hash = hash.as_str().ok_or("BATCH_STATE")?;
            let saved = self.engine.trusted_recovery_attempt(&view.commit, hash)?
                .ok_or(Error::Recovery("ATTEMPT_NOT_FOUND"))?;
            let a = &saved.attempt;
            if a["batch"] != b["batch"] || a["context"] != *s.context() {
                return Err(Error::Recovery("ATTEMPT_CONFLICT"));
            }
            let close = match a["kind"].as_str() {
                Some("SETTLE") => { settles += 1; false }
                Some("CLOSE") => { closes += 1; true }
                _ => return Err(Error::Recovery("ATTEMPT_KIND")),
            };
            match a["state"].as_str() {
                Some("PREPARED" | "SUBMISSION_UNKNOWN") => {
                    if pending.replace(BatchProgress::Pending(hash.into())).is_some() {
                        return Err(Error::Recovery("MULTIPLE_PENDING_ATTEMPTS"));
                    }
                }
                Some("INCLUDED_SUCCESS") => {
                    let r = reference(&canonical(a)?, TYPED)?;
                    let index = refs.iter().position(|v| *v == r).ok_or("ATTEMPT_NOT_FOUND")?;
                    let action = if close { BatchProgress::VoidReceipt(index) }
                        else { BatchProgress::CommittedReceipt(index) };
                    if success.replace(action).is_some() { return Err(Error::Recovery("MULTIPLE_SUCCESS_ATTEMPTS")); }
                }
                Some("INCLUDED_FAILURE") => { if !close { failed_settle = true; } }
                Some("EXPIRED_ABSENT_PROVEN") => {}
                _ => return Err(Error::Recovery("ATTEMPT_STATE")),
            }
        }
        let action = if let Some(pending) = pending { pending }
        else if let Some(success) = success { success }
        else if matches!(state, "REJECTED_FINAL" | "CLOSING") {
            if closes >= 2 { return Err(Error::Invalid("RETRY_BUDGET_EXHAUSTED")); }
            BatchProgress::PrepareClose { batch: b["batch"]["batch_id"].as_str().ok_or("BATCH_STATE")?.into(), attempt: closes + 1 }
        } else if failed_settle { BatchProgress::RejectFinal }
        else {
            if settles >= 3 { return Err(Error::Invalid("RETRY_BUDGET_EXHAUSTED")); }
            BatchProgress::PrepareSettle { batch: b["batch"]["batch_id"].as_str().ok_or("BATCH_STATE")?.into(), attempt: settles + 1 }
        };
        Ok((view.commit.clone(), Some(action)))
    }
    /// Exactly one existing action; no loop, Seal, Apply or receipt synthesis.
    pub fn active_tick(&mut self, chain: &ChainRead, rpc: &LoopbackRpc,
        s: &Snapshot, signer: &impl OperatorSigner, o: &Observation)
        -> Result<Option<BatchProgress>> {
        self.active_tick_with(s, o, now, |lane, action| {
            match action {
                BatchProgress::Pending(hash) => { lane.pending_tick(chain, rpc, s, hash, o)?; }
                BatchProgress::CommittedReceipt(index) => lane.committed_receipt(chain, s, *index, o)?,
                BatchProgress::VoidReceipt(index) => lane.void_receipt(chain, s, *index, o)?,
                BatchProgress::RejectFinal => lane.reject_final(s, o)?,
                BatchProgress::PrepareSettle { batch, attempt } => { lane.prepare(chain, s, batch, *attempt, signer, o)?; }
                BatchProgress::PrepareClose { batch, attempt } => { lane.prepare_close(chain, s, batch, *attempt, signer, o)?; }
            }
            Ok(())
        })
    }
    fn active_tick_with(&mut self, s: &Snapshot, o: &Observation,
        clock: impl FnOnce() -> Result<u64>,
        effect: impl FnOnce(&mut Self, &BatchProgress) -> Result<()>) -> Result<Option<BatchProgress>> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        let (commit, action) = self.active_action(s, o, clock()?)?;
        if self.engine.reader().get()?.commit != commit { return Err(Error::Invalid("STALE_COMMIT")); }
        self.closed = false;
        let Some(action) = action else { return Ok(None); };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| effect(self, &action)));
        match result {
            Ok(Ok(())) => Ok(Some(action)),
            Ok(Err(e)) => { self.closed = true; Err(e) }
            Err(panic) => { self.closed = true; std::panic::resume_unwind(panic) }
        }
    }
    /// Resolve an active batch before applying a newer ledger observation.
    /// C selects Apply/Seal readiness at the same committed revision.
    pub fn reconcile_tick(&mut self, chain: &ChainRead, rpc: &LoopbackRpc,
        s: &Snapshot, signer: &impl OperatorSigner, o: &Observation)
        -> Result<ReconcileProgress> {
        self.reconcile_tick_with(s, o, now, |lane, action| match action {
            ReconcileProgress::Active(expected) => {
                if lane.active_tick(chain, rpc, s, signer, o)?.as_ref() != Some(expected) {
                    return Err(Error::Invalid("DISPATCH_CONFLICT"));
                }
                Ok(())
            }
            ReconcileProgress::Apply => lane.apply(s, o),
            ReconcileProgress::Seal(purpose) => lane.seal(s, purpose.as_str(), o),
            ReconcileProgress::Idle => Err(Error::Invalid("IDLE_EFFECT")),
        })
    }
    fn reconcile_tick_with(&mut self, s: &Snapshot, o: &Observation,
        clock: impl FnOnce() -> Result<u64>,
        effect: impl FnOnce(&mut Self, &ReconcileProgress) -> Result<()>) -> Result<ReconcileProgress> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        let at = clock()?;
        let (commit, active) = self.active_action(s, o, at)?;
        let ready = self.engine.trusted_reconcile_readiness(&commit, o, at)?;
        let view = self.engine.reader().get()?;
        if view.commit != commit { return Err(Error::Invalid("STALE_COMMIT")); }
        let action = if let Some(active) = active { ReconcileProgress::Active(active) }
            else if ready.apply == ApplyReadiness::Ready { ReconcileProgress::Apply }
            else { match ready.seal {
                SealReadiness::Ready(purpose) => ReconcileProgress::Seal(purpose),
                SealReadiness::Waiting(_) => ReconcileProgress::Idle,
                SealReadiness::ActiveBatch { .. } => return Err(Error::Invalid("DISPATCH_CONFLICT")),
            } };
        if action == ReconcileProgress::Idle { self.closed = false; return Ok(action); }
        self.closed = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| effect(self, &action)));
        match result {
            Ok(Ok(())) => Ok(action),
            Ok(Err(e)) => { self.closed = true; Err(e) }
            Err(panic) => { self.closed = true; std::panic::resume_unwind(panic) }
        }
    }
    /// Exactly one Account query and one prepare call; no broadcast or retry.
    /// Preserve the observer's timestamp, including time spent querying Account.
    pub fn prepare(
        &mut self,
        chain: &ChainRead,
        s: &Snapshot,
        batch: &str,
        attempt: u64,
        signer: &impl OperatorSigner,
        o: &Observation,
    ) -> Result<Prepared> {
        self.prepare_with(
            s,
            batch,
            attempt,
            signer,
            o,
            |s, owner| chain.account(s, owner),
            now,
        )
    }
    fn prepare_with(
        &mut self,
        s: &Snapshot,
        batch: &str,
        attempt: u64,
        signer: &impl OperatorSigner,
        o: &Observation,
        fetch: impl FnOnce(&Snapshot, &[u8]) -> Result<Account>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<Prepared> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true; // Error/unwind requires explicit restart from durable state.
        self.bound(s, o, clock()?)?;
        let owner = schema::bytes(&s.value()["operator"])?;
        let account = fetch(s, &owner)?;
        let (number, sequence) = account.at(s, &owner)?;
        let after_query = clock()?;
        self.bound(s, o, after_query)?;
        let hash = self.worker.prepare_settle(
            s,
            batch,
            attempt,
            number,
            sequence,
            signer,
            o,
            after_query,
        )?;
        self.closed = false;
        Ok(Prepared {
            tx_hash: hash,
            account_rpc: account.raw().to_vec(),
        })
    }
    /// CLOSE uses C's original failure at the commit captured before Account IO.
    /// Only L-D constructs/signs the envelope; this does not broadcast or Apply.
    pub fn prepare_close(
        &mut self,
        chain: &ChainRead,
        s: &Snapshot,
        batch: &str,
        attempt: u64,
        signer: &impl OperatorSigner,
        o: &Observation,
    ) -> Result<Prepared> {
        self.prepare_close_with(s, batch, attempt, signer, o,
            |s, owner| chain.account(s, owner), now)
    }
    fn prepare_close_with(
        &mut self,
        s: &Snapshot,
        batch: &str,
        attempt: u64,
        signer: &impl OperatorSigner,
        o: &Observation,
        fetch: impl FnOnce(&Snapshot, &[u8]) -> Result<Account>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<Prepared> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let expected = self.engine.reader().get()?.commit.clone();
        let owner = schema::bytes(&s.value()["operator"])?;
        let account = fetch(s, &owner)?;
        let (number, sequence) = account.at(s, &owner)?;
        let after_query = clock()?;
        self.bound(s, o, after_query)?;
        let hash = self.worker.prepare_close(&expected, s, batch, attempt,
            number, sequence, signer, o, after_query)?;
        self.closed = false;
        Ok(Prepared { tx_hash: hash, account_rpc: account.raw().to_vec() })
    }
    /// Inspect one trusted observed height. Missing TX is not terminal evidence.
    /// No signing, broadcasting, receipt/Apply or correction occurs here.
    pub fn resolve_inclusion(
        &mut self,
        chain: &ChainRead,
        s: &Snapshot,
        hash: &str,
        o: &Observation,
    ) -> Result<bool> {
        self.resolve_inclusion_with(s, hash, o, |s, tx| chain.confirmed(s, tx), now)
    }
    /// Inspect one persisted historical height after restart/catch-up. Current
    /// observation supplies freshness; history never becomes a fresh observation.
    /// A miss is not absence proof. The dispatcher must scan the remaining window.
    pub fn resolve_historical_inclusion(
        &mut self, chain: &ChainRead, current: &Snapshot, hash: &str,
        height: u64, o: &Observation,
    ) -> Result<bool> {
        self.resolve_at_with(current, hash, height, o,
            |historical, tx| chain.confirmed(historical, tx), now)
    }
    /// One tick scans at most one persisted height. Restart or hash change
    /// replays from first_possible_height; only successful misses advance RAM.
    /// WindowScanned authorizes nothing: absence requires its separate full proof.
    pub fn scan_inclusion(
        &mut self, chain: &ChainRead, current: &Snapshot, hash: &str,
        o: &Observation,
    ) -> Result<InclusionProgress> {
        self.scan_inclusion_with(current, hash, o,
            |historical, tx| chain.confirmed(historical, tx), now)
    }
    fn scan_inclusion_with(
        &mut self, s: &Snapshot, hash: &str, o: &Observation,
        fetch: impl FnOnce(&Snapshot, &[u8]) -> Result<Option<(serde_json::Value, Objects)>>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<InclusionProgress> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let view = self.engine.reader().get()?;
        let saved = self.engine.trusted_recovery_attempt(&view.commit, hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        let a = &saved.attempt;
        if !matches!(a["state"].as_str(), Some("PREPARED" | "SUBMISSION_UNKNOWN")) {
            return Err(Error::Invalid("ATTEMPT_TERMINAL"));
        }
        let first = schema::num(&a["first_possible_height"])?;
        let last = schema::num(&a["timeout_height"])?;
        if a["context"] != *s.context() || last.checked_sub(first) != Some(7) {
            return Err(Error::Invalid("INCLUSION_WINDOW"));
        }
        let next = match &self.inclusion_next {
            Some((old, next)) if old == hash => *next,
            _ => first,
        };
        let progress = if next > last {
            InclusionProgress::WindowScanned
        } else if next > s.height() {
            InclusionProgress::Waiting
        } else {
            // resolve_at_with owns freshness, same-commit IO and C transition checks.
            self.closed = false;
            let found = self.resolve_at_with(s, hash, next, o, fetch, clock)?;
            self.closed = true;
            if found { InclusionProgress::Included(next) }
            else {
                self.inclusion_next = Some((hash.to_owned(),
                    next.checked_add(1).ok_or(Error::Invalid("INCLUSION_WINDOW"))?));
                InclusionProgress::Missing(next)
            }
        };
        self.closed = false;
        Ok(progress)
    }
    fn resolve_inclusion_with(
        &mut self,
        s: &Snapshot,
        hash: &str,
        o: &Observation,
        fetch: impl FnOnce(&Snapshot, &[u8]) -> Result<Option<(serde_json::Value, Objects)>>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<bool> {
        self.resolve_at_with(s, hash, s.height(), o, fetch, clock)
    }
    fn resolve_at_with(
        &mut self, s: &Snapshot, hash: &str, height: u64, o: &Observation,
        fetch: impl FnOnce(&Snapshot, &[u8]) -> Result<Option<(serde_json::Value, Objects)>>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<bool> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let expected = self.engine.reader().get()?.commit.clone();
        let saved = self.engine.trusted_recovery_attempt(&expected, hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        let mut attempt = saved.attempt;
        if !matches!(attempt["state"].as_str(), Some("PREPARED" | "SUBMISSION_UNKNOWN")) {
            return Err(Error::Invalid("ATTEMPT_TERMINAL"));
        }
        if attempt["context"] != *s.context() || height > s.height()
            || height < schema::num(&attempt["first_possible_height"])?
            || height > schema::num(&attempt["timeout_height"])? {
            return Err(Error::Invalid("INCLUSION_HEIGHT"));
        }
        let page = self.engine.trusted_recovery_history(&expected, Some(height), 1)?;
        let historical = &page.observations.first()
            .ok_or(Error::Recovery("INCLUSION_HISTORY_MISSING"))?.snapshot;
        if historical.height() != height || historical.context() != s.context()
            || page.latest.snapshot != *s {
            return Err(Error::Recovery("INCLUSION_HISTORY_CONFLICT"));
        }
        let tx = saved.evidence.resolve(&attempt["raw_tx_ref"],
            nus_exchange_contract::s3::evidence::TX)?;
        let found = fetch(historical, tx)?;
        self.bound(s, o, clock()?)?;
        if self.engine.reader().get()?.commit != expected {
            return Err(Error::Invalid("STALE_COMMIT"));
        }
        let Some((confirmed, objects)) = found else {
            self.closed = false;
            return Ok(false);
        };
        // C validates full raw evidence/history and the immutable envelope.
        attempt["state"] = serde_json::json!(if confirmed["abci_code"] == "0" {
            "INCLUDED_SUCCESS"
        } else {
            "INCLUDED_FAILURE"
        });
        attempt["confirmed_tx"] = confirmed;
        let evidence = objects
            .entries()
            .map(|(r, raw)| {
                Ok((
                    raw.to_vec(),
                    r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?.into(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.worker
            .reconcile(Command::Resolve(attempt), &evidence, o, clock()?)?;
        self.closed = false;
        Ok(true)
    }
    /// Prove the complete timeout window through trusted RPC, then ask C to
    /// validate it against its own durable history. This never releases assets.
    pub fn resolve_absence(
        &mut self,
        chain: &ChainRead,
        s: &Snapshot,
        hash: &str,
        o: &Observation,
    ) -> Result<()> {
        let engine = self.engine.clone();
        self.resolve_absence_with(
            s,
            hash,
            o,
            |s, a| {
                let mut cursor = recovery::RecoveryCursor::open(engine)?;
                let page = cursor.timeout_history(hash)?;
                if page.latest.snapshot != *s {
                    return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
                }
                let history = page.observations.iter().map(|row| &row.snapshot).collect::<Vec<_>>();
                let owner = schema::bytes(&a["operator"])?;
                let account = chain.account(s, &owner)?;
                chain.absence_with_account(s, &history, a, &account)
            },
            now,
        )
    }
    fn resolve_absence_with(
        &mut self,
        s: &Snapshot,
        hash: &str,
        o: &Observation,
        fetch: impl FnOnce(&Snapshot, &serde_json::Value) -> Result<(serde_json::Value, Objects)>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let mut attempt = self
            .engine
            .committed_attempt(hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        if attempt["context"] != *s.context()
            || s.height() <= schema::num(&attempt["timeout_height"])?
            || !matches!(
                attempt["state"].as_str(),
                Some("PREPARED" | "SUBMISSION_UNKNOWN")
            )
        {
            return Err(Error::Invalid("ABSENCE_POLICY"));
        }
        let (proof, objects) = fetch(s, &attempt)?;
        self.bound(s, o, clock()?)?;
        attempt["state"] = serde_json::json!("EXPIRED_ABSENT_PROVEN");
        attempt["absence_proof"] = proof;
        let evidence = objects
            .entries()
            .map(|(r, raw)| {
                Ok((
                    raw.to_vec(),
                    r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?.into(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.worker
            .reconcile(Command::Resolve(attempt), &evidence, o, clock()?)?;
        self.closed = false;
        Ok(())
    }
    /// Recover a successful SETTLE's exact TX/history from one C commit, then
    /// collect and persist its receipt. Applying balances remains a separate call.
    pub fn committed_receipt(
        &mut self, chain: &ChainRead, s: &Snapshot, index: usize, o: &Observation,
    ) -> Result<()> {
        self.committed_receipt_with(s, index, o,
            |s, terminal, batch, tx| chain.committed_receipt(s, terminal, batch, tx), now)
    }
    fn committed_receipt_with(
        &mut self, s: &Snapshot, index: usize, o: &Observation,
        fetch: impl FnOnce(&Snapshot, &Snapshot, &serde_json::Value, &[u8])
            -> Result<(serde_json::Value, Objects)>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let mut cursor = recovery::RecoveryCursor::open(self.engine.clone())?;
        let recovered = cursor.attempt_at(index)?.ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        let a = &recovered.attempt;
        if a["context"] != *s.context() || a["kind"] != "SETTLE"
            || a["state"] != "INCLUDED_SUCCESS" {
            return Err(Error::Invalid("RECEIPT_ATTEMPT"));
        }
        let h = schema::num(&a["confirmed_tx"]["height"])?;
        let page = cursor.history(Some(h), 1)?;
        let terminal = &page.observations.first().ok_or(Error::Invalid("PROOF_HISTORY_GAP"))?.snapshot;
        if terminal.height() != h || h > s.height() { return Err(Error::Invalid("RECEIPT_HISTORY")); }
        let tx = recovered.evidence.resolve(&a["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX)?;
        let (receipt, objects) = fetch(s, terminal, &a["batch"], tx)?;
        self.bound(s, o, clock()?)?;
        if self.engine.reader().get()?.commit != recovered.commit {
            return Err(Error::Invalid("STALE_COMMIT"));
        }
        if receipt["disposition"] != "COMMITTED" || receipt["batch"] != a["batch"]
            || receipt["terminal_tx"] != a["confirmed_tx"] {
            return Err(Error::Invalid("RECEIPT_INCONSISTENCY"));
        }
        let evidence = objects.entries().map(|(r, raw)| Ok((raw.to_vec(),
            r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?.into())))
            .collect::<Result<Vec<_>>>()?;
        self.worker.reconcile(Command::Receipt(receipt), &evidence, o, clock()?)?;
        self.closed = false;
        Ok(())
    }
    /// Persist VOID only from a successful CLOSE and C's exact saved failure.
    /// Correction and balance release remain exclusively in C's Apply transition.
    pub fn void_receipt(
        &mut self, chain: &ChainRead, s: &Snapshot, index: usize, o: &Observation,
    ) -> Result<()> {
        self.void_receipt_with(s, index, o,
            |s, terminal, batch, tx, failure| chain.void_receipt(s, terminal, batch,
                tx, &failure.resolution_evidence_ref, &failure.evidence), now)
    }
    fn void_receipt_with(
        &mut self, s: &Snapshot, index: usize, o: &Observation,
        fetch: impl FnOnce(&Snapshot, &Snapshot, &serde_json::Value, &[u8],
            &nus_exchange_contract::s3::dev_local::RecoveryFailure)
            -> Result<(serde_json::Value, Objects)>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let mut cursor = recovery::RecoveryCursor::open(self.engine.clone())?;
        let recovered = cursor.attempt_at(index)?.ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        let a = &recovered.attempt;
        if a["context"] != *s.context() || a["kind"] != "CLOSE"
            || a["state"] != "INCLUDED_SUCCESS" {
            return Err(Error::Invalid("VOID_ATTEMPT"));
        }
        let failure = cursor.failure(a["batch"]["batch_id"].as_str().ok_or("BATCH_ID")?)?
            .ok_or(Error::Invalid("VOID_FAILURE_MISSING"))?;
        if failure.commit != recovered.commit { return Err(Error::Invalid("STALE_COMMIT")); }
        let h = schema::num(&a["confirmed_tx"]["height"])?;
        let page = cursor.history(Some(h), 1)?;
        let terminal = &page.observations.first().ok_or(Error::Invalid("PROOF_HISTORY_GAP"))?.snapshot;
        if terminal.height() != h || h > s.height() { return Err(Error::Invalid("RECEIPT_HISTORY")); }
        let tx = recovered.evidence.resolve(&a["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX)?;
        let (receipt, objects) = fetch(s, terminal, &a["batch"], tx, &failure)?;
        self.bound(s, o, clock()?)?;
        if self.engine.reader().get()?.commit != recovered.commit {
            return Err(Error::Invalid("STALE_COMMIT"));
        }
        if receipt["disposition"] != "VOID" || receipt["batch"] != a["batch"]
            || receipt["terminal_tx"] != a["confirmed_tx"]
            || receipt["resolution_evidence_ref"] != failure.resolution_evidence_ref {
            return Err(Error::Invalid("RECEIPT_INCONSISTENCY"));
        }
        let evidence = objects.entries().map(|(r, raw)| Ok((raw.to_vec(),
            r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?.into())))
            .collect::<Result<Vec<_>>>()?;
        self.worker.reconcile(Command::Receipt(receipt), &evidence, o, clock()?)?;
        self.closed = false;
        Ok(())
    }
    /// Ask C to derive and validate final rejection from its persisted attempts
    /// and latest observation. This does not create CLOSE, VOID receipt or Apply.
    pub fn reject_final(&mut self, s: &Snapshot, o: &Observation) -> Result<()> {
        self.reject_final_with(s, o, now)
    }
    fn reject_final_with(
        &mut self, s: &Snapshot, o: &Observation,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<()> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        let at = clock()?;
        self.bound(s, o, at)?;
        self.worker.reconcile(Command::RejectFinal, &[], o, at)?;
        self.closed = false;
        Ok(())
    }
    /// Trusted driver supplies the purpose. C alone chooses FIFO fills and
    /// validates expiry/epoch/failure conditions. No fallback on rejection.
    pub fn seal(&mut self, s: &Snapshot, purpose: &str, o: &Observation) -> Result<()> {
        self.seal_with(s, purpose, o, now)
    }
    fn seal_with(&mut self, s: &Snapshot, purpose: &str, o: &Observation,
        clock: impl FnOnce() -> Result<u64>) -> Result<()> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        let at = clock()?;
        self.bound(s, o, at)?;
        self.worker.reconcile(Command::Seal(purpose.into()), &[], o, at)?;
        self.closed = false;
        Ok(())
    }
    /// Apply only C's already persisted observations and terminal evidence.
    /// No RPC, signing, receipt synthesis or economic logic lives in this lane.
    pub fn apply(&mut self, s: &Snapshot, o: &Observation) -> Result<()> {
        self.apply_with(s, o, now)
    }
    fn apply_with(
        &mut self,
        s: &Snapshot,
        o: &Observation,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<()> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true;
        let at = clock()?;
        self.bound(s, o, at)?;
        self.worker.reconcile(Command::Apply, &[], o, at)?;
        self.closed = false;
        Ok(())
    }
    /// Only a persisted exact hash can be broadcast. C writes UNKNOWN/count
    /// before bounded IO under its writer gate. No signer or replacement here.
    pub fn broadcast_existing(
        &mut self,
        hash: &str,
        rpc: &LoopbackRpc,
        s: &Snapshot,
        o: &Observation,
    ) -> Result<serde_json::Value> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true;
        let time = now()?;
        self.bound(s, o, time)?;
        let a = self
            .engine
            .committed_attempt(hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        if a["context"] != *s.context()
            || a["operator"] != s.value()["operator"]
            || a["operator_epoch"] != s.value()["operator_epoch"]
            || s.height() >= schema::num(&a["timeout_height"])?
        {
            return Err(Error::Invalid("ATTEMPT_POLICY"));
        }
        let result = self.worker.broadcast(hash, rpc, o, time)?;
        self.closed = false;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "../../../exchange/tests/support/dev_fixture.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, Signer},
    };
    use nus_exchange_contract::s3::{
        dev_local::{Command, Validated},
        snapshot::Binding,
    };
    use std::cell::Cell;
    struct Sign {
        pk: Vec<u8>,
        calls: Cell<usize>,
    }
    impl Sign {
        fn new() -> Self {
            Self {
                pk: hex::decode(fixture::key(16)["public_key_hex"].as_str().unwrap()).unwrap(),
                calls: Cell::new(0),
            }
        }
    }
    impl OperatorSigner for Sign {
        fn public_key(&self) -> &[u8] {
            &self.pk
        }
        fn sign(&self, doc: &[u8]) -> Result<Vec<u8>> {
            self.calls.set(self.calls.get() + 1);
            let seed: [u8; 32] = hex::decode(fixture::key(16)["test_seed_hex"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap();
            let (_, sk) = ml_dsa_65::KG::keygen_from_seed(&seed);
            Ok(sk.try_sign_with_seed(&[0; 32], doc, &[]).unwrap().to_vec())
        }
    }
    fn snapshot(v: &serde_json::Value, bps: u32) -> Snapshot {
        Binding::new(
            v["context"].clone(),
            v["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| schema::bytes(&a["owner"]).unwrap())
                .collect(),
            [4_000_000_000_000; 2],
            bps,
        )
        .unwrap()
        .decode(&canonical(v).unwrap())
        .unwrap()
    }
    fn account(s: &Snapshot, owner: &[u8]) -> Result<Account> {
        let pk = hex::decode(fixture::key(16)["public_key_hex"].as_str().unwrap()).unwrap();
        let raw =
            collect::account::tests::rpc(s, &collect::account::tests::base(owner, &pk, 16, 0));
        collect::account::decode(s, owner, &raw)
    }
    fn setup(
        bps: u32,
    ) -> (
        Arc<Engine>,
        Snapshot,
        String,
        nus_exchange_contract::s3::dev_local::Inputs,
        std::path::PathBuf,
    ) {
        let (inputs, v) = fixture::initial(bps);
        let home = fixture::home(bps);
        let e = Arc::new(
            Engine::create(
                &home,
                Validated::new(inputs.clone()).unwrap(),
                &canonical(&v).unwrap(),
            )
            .unwrap(),
        );
        let o = fixture::observation(&v);
        for (i, side, id) in [(0, "2", 241), (1, "1", 242)] {
            let (raw, sig) = fixture::sign_order(&v, i, side, 1000, 10000, id);
            e.execute(fixture::signed(&raw, &sig, i), &[], &o, fixture::NOW)
                .unwrap();
        }
        e.execute(Command::Seal("NORMAL".into()), &[], &o, fixture::NOW)
            .unwrap();
        let id = e.reader().get().unwrap().state["batches"][0]["batch"]["batch_id"]
            .as_str()
            .unwrap()
            .to_owned();
        (e, snapshot(&v, bps), id, inputs, home)
    }
    fn unsealed(bps: u32) -> (Arc<Engine>, Snapshot,
        nus_exchange_contract::s3::dev_local::Inputs, std::path::PathBuf) {
        let (inputs, v) = fixture::initial(bps);
        let home = fixture::home(bps);
        let e = Arc::new(Engine::create(&home, Validated::new(inputs.clone()).unwrap(),
            &canonical(&v).unwrap()).unwrap());
        let o = fixture::observation(&v);
        for (i, side, id) in [(0, "2", 241), (1, "1", 242)] {
            let (raw, sig) = fixture::sign_order(&v, i, side, 1000, 10000, id);
            e.execute(fixture::signed(&raw, &sig, i), &[], &o, fixture::NOW).unwrap();
        }
        (e, snapshot(&v,bps), inputs, home)
    }
    #[test]
    fn readiness_dispatch_epoch_change_selects_resolve_and_replays() {
        for bps in [0,25] {
            let (e,s,inputs,home) = unsealed(bps);
            let mut v = s.value().clone();
            v["height"] = serde_json::json!("101");
            v["operator_epoch"] = serde_json::json!("2");
            fixture::finish(&mut v);
            let o = fixture::observation(&v);
            e.execute(Command::Snapshot(canonical(&v).unwrap()), &[], &o, fixture::NOW).unwrap();
            let s = snapshot(&v,bps);
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            assert_eq!(lane.reconcile_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                assert_eq!(*a,ReconcileProgress::Seal(SealPurpose::ResolveFailure));
                lane.seal_with(&s,"RESOLVE_FAILURE",&o,|| Ok(fixture::NOW))
            }).unwrap(),ReconcileProgress::Seal(SealPurpose::ResolveFailure));
            assert!(!lane.closed);
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["accounts"],before.state["accounts"]);
            assert_eq!(after.state["attempt_refs"],before.state["attempt_refs"]);
            assert_eq!(after.state["batches"][0]["seal_purpose"],"RESOLVE_FAILURE");
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state,after.state);
                assert_eq!(e.reader().get().unwrap().commit,after.commit);
            }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn readiness_dispatch_normal_seal_is_one_action_and_replays() {
        for bps in [0,25] {
            let (e,s,inputs,home) = unsealed(bps);
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            let o = fixture::observation(s.value());
            assert_eq!(lane.reconcile_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                assert_eq!(*a,ReconcileProgress::Seal(SealPurpose::Normal));
                lane.seal_with(&s,"NORMAL",&o,|| Ok(fixture::NOW))
            }).unwrap(),ReconcileProgress::Seal(SealPurpose::Normal));
            let after = e.reader().get().unwrap();
            assert_eq!(after.commit.command_seq,before.commit.command_seq+1);
            assert_eq!(after.state["accounts"],before.state["accounts"]);
            assert_eq!(after.state["attempt_refs"],before.state["attempt_refs"]);
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state,after.state);
                assert_eq!(e.reader().get().unwrap().commit,after.commit);
            }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn readiness_dispatch_applies_then_requeries_before_seal() {
        for bps in [0,25] {
            let (e,s,inputs,home) = unsealed(bps);
            let mut v = s.value().clone();
            v["height"] = serde_json::json!("101");
            fixture::finish(&mut v);
            let o = fixture::observation(&v);
            e.execute(Command::Snapshot(canonical(&v).unwrap()), &[], &o, fixture::NOW).unwrap();
            let s = snapshot(&v,bps);
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            assert_eq!(lane.reconcile_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                assert_eq!(*a,ReconcileProgress::Apply);
                lane.apply_with(&s,&o,|| Ok(fixture::NOW))
            }).unwrap(),ReconcileProgress::Apply);
            let applied = e.reader().get().unwrap();
            assert_eq!(applied.commit.command_seq,before.commit.command_seq+1);
            assert_eq!(applied.state["batches"],before.state["batches"]);
            assert!(e.trusted_reconcile_readiness(&before.commit,&o,fixture::NOW).is_err());
            drop(lane); drop(e);
            let e = Arc::new(Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap());
            let mut lane = SubmitLane::new(e.clone());
            assert_eq!(lane.reconcile_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                assert_eq!(*a,ReconcileProgress::Seal(SealPurpose::Normal));
                lane.seal_with(&s,"NORMAL",&o,|| Ok(fixture::NOW))
            }).unwrap(),ReconcileProgress::Seal(SealPurpose::Normal));
            let after = e.reader().get().unwrap();
            assert_eq!(after.commit.command_seq,applied.commit.command_seq+1);
            assert_eq!(after.state["accounts"],applied.state["accounts"]);
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state,after.state);
            }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn reconcile_dispatch_receipt_then_apply_then_idle_and_replay() {
        for bps in [0,25] { for void in [false,true] {
            let (e,mut lane,s,inputs,home) = if void {void_ready(bps)} else {terminal_ready(bps)};
            let o = fixture::observation(s.value());
            let before = e.reader().get().unwrap();
            let action = lane.reconcile_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                match a {
                    ReconcileProgress::Active(BatchProgress::VoidReceipt(i)) if void =>
                        lane.void_receipt_with(&s,*i,&o,void_input,|| Ok(fixture::NOW)),
                    ReconcileProgress::Active(BatchProgress::CommittedReceipt(i)) if !void =>
                        lane.committed_receipt_with(&s,*i,&o,receipt_input,|| Ok(fixture::NOW)),
                    _ => panic!("receipt must precede apply"),
                }
            }).unwrap();
            assert!(matches!(action,ReconcileProgress::Active(_)));
            assert_eq!(before.state["accounts"],e.reader().get().unwrap().state["accounts"]);
            drop(lane); drop(e);
            let e = Arc::new(Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap());
            let mut lane = SubmitLane::new(e.clone());
            assert_eq!(lane.reconcile_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                assert_eq!(*a,ReconcileProgress::Apply);
                lane.apply_with(&s,&o,|| Ok(fixture::NOW))
            }).unwrap(),ReconcileProgress::Apply);
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["chain_snapshot"],*s.value());
            assert_eq!(after.state["batches"][0]["state"],if void {"CORRECTED"} else {"COMMITTED"});
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap());
                let mut lane = SubmitLane::new(e.clone());
                assert_eq!(lane.reconcile_tick_with(&s,&o,|| Ok(fixture::NOW),|_,_| panic!("repeat effect")).unwrap(),ReconcileProgress::Idle);
                assert_eq!(e.reader().get().unwrap().state,after.state);
                assert_eq!(e.reader().get().unwrap().commit,after.commit);
            }
            std::fs::remove_dir_all(home).unwrap();
        }}
    }
    #[test]
    fn reconcile_dispatch_error_panic_and_stale_close_lane() {
        for mode in 0..3 {
            let (e,s,_,_,home) = setup(0);
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            let calls = Cell::new(0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                lane.reconcile_tick_with(&s,&fixture::observation(s.value()),
                    || Ok(fixture::NOW+if mode == 0 {6000} else {0}),|_,_| {
                    calls.set(calls.get()+1);
                    if mode == 2 {panic!("injected");}
                    Err(Error::Invalid("IO"))
                })));
            assert!(matches!(result,Err(_) | Ok(Err(_))));
            assert_eq!(calls.get(),if mode == 0 {0} else {1});
            assert!(lane.reconcile_tick_with(&s,&fixture::observation(s.value()),
                || panic!("closed clock"),|_,_| panic!("closed effect")).is_err());
            assert_eq!(e.reader().get().unwrap().commit,before.commit);
            assert_eq!(e.reader().get().unwrap().state,before.state);
            drop(lane); drop(e); std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn seal_lane_persists_only_batch_and_replays_twice() {
        for bps in [0,25] {
            let (e,s,inputs,home) = unsealed(bps);
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            lane.seal_with(&s,"NORMAL",&fixture::observation(s.value()),|| Ok(fixture::NOW)).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(before.state["accounts"],after.state["accounts"]);
            assert_eq!(after.state["batches"].as_array().unwrap().len(),1);
            assert_eq!(after.state["attempt_refs"].as_array().unwrap().len(),0);
            assert!(lane.seal_with(&s,"NORMAL",&fixture::observation(s.value()),|| Ok(fixture::NOW)).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit,after.commit);
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state,after.state);
                assert_eq!(e.reader().get().unwrap().commit,after.commit);
            }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn seal_lane_rejection_never_falls_back_or_changes_commit() {
        for bps in [0,25] { for purpose in ["RESOLVE_FAILURE","unknown"] {
            let (e,s,_,home) = unsealed(bps);
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            assert!(lane.seal_with(&s,purpose,&fixture::observation(s.value()),|| Ok(fixture::NOW)).is_err());
            assert!(lane.closed);
            assert!(lane.seal_with(&s,"NORMAL",&fixture::observation(s.value()),|| panic!("closed lane clock")).is_err());
            assert_eq!(e.reader().get().unwrap().commit,before.commit);
            assert_eq!(e.reader().get().unwrap().state,before.state);
            drop(lane); drop(e); std::fs::remove_dir_all(home).unwrap();
        }}
    }
    #[test]
    fn seal_lane_stale_or_clock_failure_preserves_store() {
        for bps in [0,25] { for stale in [false,true] {
            let (e,s,_,home) = unsealed(bps);
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            assert!(lane.seal_with(&s,"NORMAL",&fixture::observation(s.value()),||
                if stale { Ok(fixture::NOW+60_000) } else { Err(Error::Invalid("CLOCK")) }).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit,before.commit);
            assert_eq!(e.reader().get().unwrap().state,before.state);
            drop(lane); drop(e); std::fs::remove_dir_all(home).unwrap();
        }}
    }
    #[test]
    fn active_dispatch_prepares_once_then_recovers_pending() {
        for bps in [0,25] {
            let (e, s, id, inputs, home) = setup(bps);
            let o = fixture::observation(s.value());
            let mut lane = SubmitLane::new(e.clone());
            let before = e.reader().get().unwrap();
            let sign = Sign::new();
            let action = lane.active_tick_with(&s, &o, || Ok(fixture::NOW), |lane,a| {
                assert_eq!(*a, BatchProgress::PrepareSettle { batch: id.clone(), attempt: 1 });
                lane.prepare_with(&s, &id, 1, &sign, &o, account, || Ok(fixture::NOW))?;
                Ok(())
            }).unwrap();
            assert!(matches!(action, Some(BatchProgress::PrepareSettle { attempt: 1, .. })));
            assert_eq!(sign.calls.get(), 1);
            let after = e.reader().get().unwrap();
            assert_eq!(before.state["accounts"], after.state["accounts"]);
            let hash = after.state["batches"][0]["attempt_hashes"][0].as_str().unwrap().to_owned();
            let a = e.trusted_recovery_attempt(&after.commit, &hash).unwrap().unwrap();
            assert_eq!(a.attempt["broadcast_count"], "0");
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                let mut lane = SubmitLane::new(e.clone());
                let calls = Cell::new(0);
                assert_eq!(lane.active_tick_with(&s, &o, || Ok(fixture::NOW), |_,a| {
                    calls.set(calls.get()+1); assert_eq!(*a, BatchProgress::Pending(hash.clone())); Ok(())
                }).unwrap(), Some(BatchProgress::Pending(hash.clone())));
                assert_eq!(calls.get(), 1);
                assert_eq!(e.reader().get().unwrap().state, after.state);
                assert_eq!(e.reader().get().unwrap().commit, after.commit);
            }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn active_dispatch_terminal_receipts_stop_before_apply() {
        for bps in [0,25] { for void in [false,true] {
            let (e, mut lane, s, inputs, home) = if void { void_ready(bps) } else { terminal_ready(bps) };
            let o = fixture::observation(s.value());
            let before = e.reader().get().unwrap();
            let action = lane.active_tick_with(&s, &o, || Ok(fixture::NOW), |lane,a| {
                match a {
                    BatchProgress::VoidReceipt(i) if void => lane.void_receipt_with(&s,*i,&o,void_input,|| Ok(fixture::NOW)),
                    BatchProgress::CommittedReceipt(i) if !void => lane.committed_receipt_with(&s,*i,&o,receipt_input,|| Ok(fixture::NOW)),
                    _ => panic!("wrong receipt branch"),
                }
            }).unwrap();
            assert!(action.is_some());
            let after = e.reader().get().unwrap();
            assert_ne!(before.commit, after.commit);
            assert_eq!(before.state["accounts"], after.state["accounts"]);
            assert_eq!(before.state["chain_snapshot"], after.state["chain_snapshot"]);
            assert_eq!(after.state["resolution_receipts"].as_array().unwrap().len(),1);
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                let mut lane = SubmitLane::new(e.clone());
                assert_eq!(lane.active_tick_with(&s,&o,|| Ok(fixture::NOW),|_,_| panic!("resolved effect")).unwrap(),None);
                assert_eq!(e.reader().get().unwrap().state,after.state);
                assert_eq!(e.reader().get().unwrap().commit,after.commit);
            }
            std::fs::remove_dir_all(home).unwrap();
        }}
    }
    #[test]
    fn active_dispatch_failure_then_close_uses_separate_ticks() {
        for bps in [0,25] {
            let (e, mut lane, s, hash, _, home) = prepared_at_next(bps);
            let o = fixture::observation(s.value());
            lane.resolve_inclusion_with(&s,&hash,&o,expected_failure,|| Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap();
            assert_eq!(lane.active_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                assert_eq!(*a,BatchProgress::RejectFinal);
                lane.reject_final_with(&s,&o,|| Ok(fixture::NOW))
            }).unwrap(),Some(BatchProgress::RejectFinal));
            let rejected = e.reader().get().unwrap();
            assert_eq!(rejected.state["batches"][0]["state"],"REJECTED_FINAL");
            assert_eq!(rejected.state["attempt_refs"],before.state["attempt_refs"]);
            let sign = Sign::new();
            lane.active_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                let BatchProgress::PrepareClose {batch,attempt} = a else { panic!("expected CLOSE"); };
                assert_eq!(*attempt,1);
                lane.prepare_close_with(&s,batch,*attempt,&sign,&o,account,|| Ok(fixture::NOW))?;
                Ok(())
            }).unwrap();
            assert_eq!(sign.calls.get(),1);
            let after = e.reader().get().unwrap();
            assert_eq!(before.state["accounts"],after.state["accounts"]);
            assert_eq!(after.state["attempt_refs"].as_array().unwrap().len(),2);
            drop(lane); drop(e); std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn active_dispatch_absence_retries_same_batch_without_rejection() {
        for bps in [0,25] {
            let (e, mut lane, s, hash, inputs, home, history) = expired(bps);
            let o = fixture::observation(s.value());
            lane.resolve_absence_with(&s,&hash,&o,|s,a| absent(s,a,&history),|| Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap();
            let batch = before.state["batches"][0]["batch"].clone();
            let sign = Sign::new();
            let action = lane.active_tick_with(&s,&o,|| Ok(fixture::NOW),|lane,a| {
                let BatchProgress::PrepareSettle {batch:id,attempt} = a else { panic!("absence is not rejection"); };
                assert_eq!(*attempt,2);
                assert_eq!(id,batch["batch_id"].as_str().unwrap());
                lane.prepare_with(&s,id,*attempt,&sign,&o,account,|| Ok(fixture::NOW))?;
                Ok(())
            }).unwrap();
            assert!(matches!(action,Some(BatchProgress::PrepareSettle {attempt:2,..})));
            assert_eq!(sign.calls.get(),1);
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["batches"][0]["batch"],batch);
            assert_eq!(before.state["accounts"],after.state["accounts"]);
            assert_eq!(after.state["resolution_receipts"],before.state["resolution_receipts"]);
            assert_eq!(after.state["attempt_refs"].as_array().unwrap().len(),2);
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap());
                let lane = SubmitLane::new(e.clone());
                assert!(matches!(lane.active_action(&s,&o,fixture::NOW).unwrap().1,Some(BatchProgress::Pending(_))));
                assert_eq!(e.reader().get().unwrap().state,after.state);
            }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn active_dispatch_errors_and_panics_close_without_second_effect() {
        for mode in 0..3 {
            let (e,s,_,_,home) = setup(0);
            let mut lane = SubmitLane::new(e.clone());
            let before = e.reader().get().unwrap();
            let o = fixture::observation(s.value());
            let calls = Cell::new(0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lane.active_tick_with(&s,&o,|| if mode == 0 { Err(Error::Invalid("CLOCK")) } else { Ok(fixture::NOW) },
                    |_,_| { calls.set(calls.get()+1); if mode == 2 { panic!("injected"); } Err(Error::Invalid("IO")) })
            }));
            assert!(matches!(result,Err(_) | Ok(Err(_))));
            assert_eq!(calls.get(),if mode == 0 {0} else {1});
            assert!(lane.active_tick_with(&s,&o,|| panic!("closed clock"),|_,_| panic!("closed effect")).is_err());
            assert_eq!(before.commit,e.reader().get().unwrap().commit);
            drop(lane); drop(e); std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn pending_dispatch_reads_durable_state_and_runs_one_action() {
        for bps in [0,25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            let o = fixture::observation(s.value());
            let before = e.reader().get().unwrap();
            assert_eq!(lane.pending_tick_with(&s, &hash, &o, || Ok(fixture::NOW),
                |_, action| { assert_eq!(action, PendingAction::Broadcast); Ok(PendingProgress::Broadcast) }).unwrap(), PendingProgress::Broadcast);
            // Callback is deliberately effect-free: action selection is not a broadcast.
            assert_eq!(before.commit, e.reader().get().unwrap().commit);
            let mut a = e.committed_attempt(&hash).unwrap().unwrap();
            a["state"] = serde_json::json!("SUBMISSION_UNKNOWN");
            a["broadcast_count"] = serde_json::json!("1");
            e.execute(Command::Resolve(a), &[], &o, fixture::NOW).unwrap();
            assert_eq!(lane.pending_action(&s, &hash, &o, fixture::NOW).unwrap(), PendingAction::Scan);
            lane.inclusion_next = Some((hash.clone(), 999));
            // Even a completed RAM scan cannot authorize absence before timeout.
            assert_eq!(lane.pending_action(&s, &hash, &o, fixture::NOW).unwrap(), PendingAction::Scan);
            let expected = e.reader().get().unwrap();
            drop(lane); drop(e);
            for _ in 0..2 {
                let reopened = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                assert_eq!(expected.state, reopened.reader().get().unwrap().state);
                let lane = SubmitLane::new(reopened);
                assert_eq!(lane.pending_action(&s, &hash, &o, fixture::NOW).unwrap(), PendingAction::Scan);
            }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn pending_dispatch_timeout_requires_full_absence_proof() {
        for bps in [0,25] {
            let (e, mut lane, s, hash, _, home, history) = expired(bps);
            let o = fixture::observation(s.value());
            assert_eq!(lane.pending_action(&s, &hash, &o, fixture::NOW).unwrap(), PendingAction::Scan);
            for old in &history {
                lane.scan_inclusion_with(&s, &hash, &o, |actual, _| {
                    assert_eq!(actual, old); Ok(None)
                }, || Ok(fixture::NOW)).unwrap();
            }
            let before = e.reader().get().unwrap();
            let progress = lane.pending_tick_with(&s, &hash, &o, || Ok(fixture::NOW), |lane, action| {
                assert_eq!(action, PendingAction::Absence);
                lane.resolve_absence_with(&s, &hash, &o, |s,a| absent(s,a,&history), || Ok(fixture::NOW))?;
                Ok(PendingProgress::AbsenceProven)
            }).unwrap();
            assert_eq!(progress, PendingProgress::AbsenceProven);
            let after = e.reader().get().unwrap();
            assert_ne!(before.commit, after.commit);
            assert_eq!(before.state["balances"], after.state["balances"]);
            assert!(lane.pending_tick_with(&s, &hash, &o, || Ok(fixture::NOW),
                |_, _| panic!("terminal effect")).is_err());
            drop(lane); drop(e); std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn pending_dispatch_failure_and_unwind_close_before_reentry() {
        for mode in 0..3 {
            let (e, mut lane, s, hash, _, home) = prepared_at_next(0);
            let o = fixture::observation(s.value());
            let before = e.reader().get().unwrap();
            let calls = Cell::new(0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lane.pending_tick_with(&s, &hash, &o,
                    || if mode == 0 { Err(Error::Invalid("CLOCK")) } else { Ok(fixture::NOW) },
                    |_, _| { calls.set(calls.get()+1); if mode == 2 { panic!("injected"); }
                        Err(Error::Invalid("IO")) })
            }));
            assert!(matches!(result, Err(_) | Ok(Err(_))));
            assert!(lane.pending_tick_with(&s, &hash, &o, || panic!("clock after close"),
                |_, _| panic!("effect after close")).is_err());
            assert_eq!(calls.get(), if mode == 0 { 0 } else { 1 });
            assert_eq!(before.commit, e.reader().get().unwrap().commit);
            drop(lane); drop(e); std::fs::remove_dir_all(home).unwrap();
        }
    }
    fn receipt_input(s: &Snapshot, terminal: &Snapshot, b: &serde_json::Value, tx: &[u8])
        -> Result<(serde_json::Value, Objects)> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let (confirmed, objects) = included(terminal, tx, 0)?.unwrap();
        let wire = serde_json::json!({"protocol_version":"2","chain_id":s.context()["chain_id"],
            "genesis_hash":s.context()["genesis_hash"],"market_id":s.context()["market_id"],
            "batch_seq":b["batch_seq"],"batch_id":b["batch_id"],"batch_hash":b["batch_hash"],
            "committed_height":terminal.height().to_string(),"tx_hash":confirmed["tx_hash"]});
        let bytes = nus_exchange_contract::codec::Codec::default().encode("BatchReceiptV1", &wire)?;
        Ok((serde_json::json!({"context":s.context(),"batch":b,"disposition":"COMMITTED",
            "terminal_tx":confirmed,"batch_receipt_v2":STANDARD.encode(bytes),
            "failed_tx_hash":null,"resolution_evidence_hash":null,"resolution_evidence_ref":null}), objects))
    }
    fn terminal_ready(bps: u32) -> (Arc<Engine>, SubmitLane, Snapshot,
        nus_exchange_contract::s3::dev_local::Inputs, std::path::PathBuf) {
        let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
        let a = e.committed_attempt(&hash).unwrap().unwrap();
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("102");
        v["last_batch_seq"] = a["batch"]["batch_seq"].clone();
        v["last_batch_hash"] = a["batch"]["batch_hash"].clone();
        v["terminal_batch_seqs"] = serde_json::json!([a["batch"]["batch_seq"]]);
        fixture::finish(&mut v);
        let s = snapshot(&v, bps);
        e.execute(Command::Snapshot(canonical(&v).unwrap()), &[], &fixture::observation(&v), fixture::NOW).unwrap();
        lane.resolve_inclusion_with(&s, &hash, &fixture::observation(&v),
            |s, tx| included(s, tx, 0), || Ok(fixture::NOW)).unwrap();
        (e, lane, s, inputs, home)
    }
    fn expected_failure(s: &Snapshot, tx: &[u8]) -> Result<Option<(serde_json::Value, Objects)>> {
        use nus_exchange_contract::s3::evidence::RPC;
        let (mut r, mut objects) = collect::inclusion_tests::input(s, vec![tx], serde_json::json!(1019));
        let mut raw: serde_json::Value = serde_json::from_slice(
            objects.resolve(&r["raw_results_response_ref"], RPC)?).unwrap();
        raw["result"]["txs_results"][0]["codespace"] = serde_json::json!("exchange_s3");
        r["raw_results_response_ref"] = objects.insert(&serde_json::to_vec(&raw).unwrap(), RPC)?;
        collect::confirmed_in_block(s, tx, r, objects)
    }
    #[test]
    fn rejected_batch_has_no_settle_signing_fallback_for_missing_close_adapter() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
                expected_failure, || Ok(fixture::NOW)).unwrap();
            lane.reject_final_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap();
            let batch = before.state["batches"][0]["batch"]["batch_id"].as_str().unwrap();
            let signer = Sign::new();
            assert!(matches!(lane.prepare_with(&s, batch, 2, &signer,
                &fixture::observation(s.value()), account, || Ok(fixture::NOW)),
                Err(Error::Invalid("BATCH_STATE"))));
            assert_eq!(signer.calls.get(), 0);
            assert_eq!(e.reader().get().unwrap().commit, before.commit);
            assert_eq!(e.reader().get().unwrap().state, before.state);
            assert!(matches!(lane.prepare_with(&s, batch, 2, &signer,
                &fixture::observation(s.value()), |_, _| panic!("closed IO"),
                || panic!("closed clock")), Err(Error::Recovery("SUBMIT_LANE_CLOSED"))));
            drop(lane); drop(e);
            for _ in 0..2 {
                let reopened = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(reopened.reader().get().unwrap().commit, before.commit);
                assert_eq!(reopened.reader().get().unwrap().state, before.state);
                assert!(reopened.trusted_recovery_failure(&before.commit, batch).unwrap().is_some());
            }
        }
    }
    #[test]
    fn close_from_recovered_failure_persists_without_release_and_replays() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            let o = fixture::observation(s.value());
            lane.resolve_inclusion_with(&s, &hash, &o, expected_failure, || Ok(fixture::NOW)).unwrap();
            lane.reject_final_with(&s, &o, || Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap();
            let batch = before.state["batches"][0]["batch"]["batch_id"].as_str().unwrap();
            let original = e.trusted_recovery_failure(&before.commit, batch).unwrap().unwrap();
            drop(lane); drop(e);
            let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
            let mut lane = SubmitLane::new(e.clone());
            let signer = Sign::new();
            let p = lane.prepare_close_with(&s, batch, 1, &signer, &o, account, || Ok(fixture::NOW)).unwrap();
            assert_eq!(signer.calls.get(), 1);
            let owner = schema::bytes(&s.value()["operator"]).unwrap();
            assert_eq!(p.account_rpc, account(&s, &owner).unwrap().raw());
            let after = e.reader().get().unwrap();
            let a = e.trusted_recovery_attempt(&after.commit, &p.tx_hash).unwrap().unwrap();
            assert_eq!(a.attempt["kind"], "CLOSE");
            assert_eq!(a.attempt["state"], "PREPARED");
            assert_eq!(a.attempt["broadcast_count"], "0");
            let failure = e.trusted_recovery_failure(&after.commit, batch).unwrap().unwrap();
            assert_eq!(failure.raw, original.raw);
            assert_eq!(failure.resolution_evidence_ref, original.resolution_evidence_ref);
            for k in ["accounts", "fills", "corrections", "resolution_receipts", "chain_snapshot"] {
                assert_eq!(before.state[k], after.state[k], "{k}");
            }
            assert!(lane.prepare_close_with(&s, batch, 2, &signer, &o, account, || Ok(fixture::NOW)).is_err());
            assert_eq!(signer.calls.get(), 1);
            assert_eq!(e.reader().get().unwrap().commit, after.commit);
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                let v = e.reader().get().unwrap();
                assert_eq!(v.commit, after.commit);
                assert_eq!(v.state, after.state);
                let r = e.trusted_recovery_attempt(&v.commit, &p.tx_hash).unwrap().unwrap();
                assert_eq!(r.attempt, a.attempt);
            }
        }
    }
    fn void_ready(bps: u32) -> (Arc<Engine>, SubmitLane, Snapshot,
        nus_exchange_contract::s3::dev_local::Inputs, std::path::PathBuf) {
        let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
        let o = fixture::observation(s.value());
        lane.resolve_inclusion_with(&s, &hash, &o, expected_failure, || Ok(fixture::NOW)).unwrap();
        lane.reject_final_with(&s, &o, || Ok(fixture::NOW)).unwrap();
        let batch = e.reader().get().unwrap().state["batches"][0]["batch"]["batch_id"].as_str().unwrap().to_owned();
        let p = lane.prepare_close_with(&s, &batch, 1, &Sign::new(), &o, account, || Ok(fixture::NOW)).unwrap();
        let a = e.committed_attempt(&p.tx_hash).unwrap().unwrap();
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("102");
        v["last_batch_seq"] = a["batch"]["batch_seq"].clone();
        v["last_batch_hash"] = a["batch"]["batch_hash"].clone();
        v["terminal_batch_seqs"] = serde_json::json!([a["batch"]["batch_seq"]]);
        fixture::finish(&mut v);
        let s = snapshot(&v, bps);
        let o = fixture::observation(&v);
        e.execute(Command::Snapshot(canonical(&v).unwrap()), &[], &o, fixture::NOW).unwrap();
        lane.resolve_inclusion_with(&s, &p.tx_hash, &o, |s, tx| included(s, tx, 0), || Ok(fixture::NOW)).unwrap();
        (e, lane, s, inputs, home)
    }
    fn void_input(s: &Snapshot, terminal: &Snapshot, b: &serde_json::Value, tx: &[u8],
        f: &nus_exchange_contract::s3::dev_local::RecoveryFailure) -> Result<(serde_json::Value, Objects)> {
        let (confirmed, mut objects) = included(terminal, tx, 0)?.unwrap();
        for (r, raw) in f.evidence.entries() {
            objects.insert(raw, r["media_type"].as_str().unwrap())?;
        }
        Ok((serde_json::json!({"context":s.context(),"batch":b,"disposition":"VOID",
            "terminal_tx":confirmed,"batch_receipt_v2":null,
            "failed_tx_hash":f.resolution_evidence["failed_tx_hash"],
            "resolution_evidence_hash":schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", &f.resolution_evidence)?,
            "resolution_evidence_ref":f.resolution_evidence_ref}), objects))
    }
    #[test]
    fn void_persist_after_restart_preserves_assets_and_replays_twice() {
        for bps in [0, 25] {
            let (e, lane, s, inputs, home) = void_ready(bps);
            let before = e.reader().get().unwrap();
            drop(lane); drop(e);
            let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
            let mut lane = SubmitLane::new(e.clone());
            lane.void_receipt_with(&s, 1, &fixture::observation(s.value()), void_input, || Ok(fixture::NOW)).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["resolution_receipts"][0]["disposition"], "VOID");
            for k in ["accounts", "fills", "corrections", "chain_snapshot"] {
                assert_eq!(after.state[k], before.state[k], "{k}");
            }
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                let replay = e.reader().get().unwrap();
                assert_eq!(replay.commit, after.commit);
                assert_eq!(replay.state, after.state);
            }
        }
    }
    #[test]
    fn void_wrong_attempt_refused_before_io() {
        let (e, mut lane, s, _, _) = terminal_ready(0);
        let before = e.reader().get().unwrap();
        assert!(lane.void_receipt_with(&s, 0, &fixture::observation(s.value()),
            |_, _, _, _, _| panic!("wrong kind IO"), || Ok(fixture::NOW)).is_err());
        assert!(lane.closed);
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
    }
    #[test]
    fn void_tamper_stale_io_and_commit_race_preserve_commit() {
        for mode in 0..5 {
            let (e, mut lane, s, _, _) = void_ready(25);
            let before = e.reader().get().unwrap();
            assert!(lane.void_receipt_with(&s, 1, &fixture::observation(s.value()),
                |s, t, b, tx, f| {
                    if mode == 2 { return Err(Error::Invalid("RPC_UNAVAILABLE")); }
                    let (mut r, objects) = void_input(s, t, b, tx, f)?;
                    if mode == 0 { r["resolution_evidence_ref"] = serde_json::Value::Null; }
                    if mode == 1 { r["resolution_evidence_hash"] = serde_json::json!("00".repeat(32)); }
                    if mode == 4 {
                        let mut next = s.value().clone();
                        next["height"] = serde_json::json!("103");
                        next["terminal_batch_seqs"] = serde_json::json!([]);
                        fixture::finish(&mut next);
                        e.execute(Command::Snapshot(canonical(&next)?), &[], &fixture::observation(&next), fixture::NOW)?;
                    }
                    Ok((r, objects))
                }, || Ok(if mode == 3 { fixture::NOW + 60_000 } else { fixture::NOW })).is_err());
            assert!(lane.closed);
            let after = e.reader().get().unwrap();
            if mode != 4 { assert_eq!(after.commit, before.commit); }
            else { assert_ne!(after.commit, before.commit, "race must actually commit"); }
            assert_eq!(after.state["resolution_receipts"], before.state["resolution_receipts"]);
            assert_eq!(after.state["accounts"], before.state["accounts"]);
        }
    }
    #[test]
    fn close_account_errors_stale_and_commit_race_never_sign() {
        for mode in 0..4 {
            let (e, mut lane, s, hash, _inputs, _home) = prepared_at_next(0);
            let o = fixture::observation(s.value());
            lane.resolve_inclusion_with(&s, &hash, &o, expected_failure, || Ok(fixture::NOW)).unwrap();
            lane.reject_final_with(&s, &o, || Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap();
            let batch = before.state["batches"][0]["batch"]["batch_id"].as_str().unwrap();
            let signer = Sign::new();
            let calls = Cell::new(0);
            let result = lane.prepare_close_with(&s, batch, 1, &signer, &o, |s, owner| {
                if mode == 0 { return Err(Error::Invalid("RPC_TEST")); }
                if mode == 2 {
                    // Same snapshot but another durable command must invalidate the pin.
                    e.execute(Command::Apply, &[], &o, fixture::NOW)?;
                }
                if mode == 3 {
                    let mut v = s.value().clone();
                    v["height"] = serde_json::json!("103");
                    fixture::finish(&mut v);
                    return account(&snapshot(&v, 0), owner);
                }
                account(s, owner)
            }, || { calls.set(calls.get()+1); Ok(fixture::NOW +
                if mode == 1 && calls.get() > 1 { 10000 } else { 0 }) });
            assert!(result.is_err(), "mode {mode}");
            assert_eq!(signer.calls.get(), 0);
            if mode == 2 {
                assert_ne!(e.reader().get().unwrap().commit, before.commit);
                assert!(matches!(result, Err(Error::Invalid("STALE_COMMIT"))));
            } else { assert_eq!(e.reader().get().unwrap().commit, before.commit); }
            assert_eq!(e.reader().get().unwrap().state["attempt_refs"], before.state["attempt_refs"]);
            assert!(lane.closed);
            assert!(lane.prepare_close_with(&s, batch, 1, &signer, &o,
                |_, _| panic!("closed IO"), || panic!("closed clock")).is_err());
        }
    }
    #[test]
    fn final_rejection_persists_c_evidence_without_asset_release_and_replays() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
                |s, tx| expected_failure(s, tx), || Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap();
            lane.reject_final_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW)).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["batches"][0]["state"], "REJECTED_FINAL");
            // Failure bytes stay private and use the dedicated commit-pinned API.
            assert!(after.state.get("failure_evidence").is_none());
            let recovered = e.trusted_recovery_attempt_at(&after.commit, 0).unwrap().unwrap();
            assert!(recovered.evidence.entries().all(|(_, raw)|
                serde_json::from_slice::<serde_json::Value>(raw).ok()
                    .is_none_or(|v| v.get("rejection_code").is_none())));

            for k in ["accounts", "fills", "chain_snapshot", "corrections", "resolution_receipts", "attempt_refs"] {
                assert!(!before.state[k].is_null(), "missing {k}");
                assert_eq!(before.state[k], after.state[k], "{k}");
            }
            let batch = after.state["batches"][0]["batch"]["batch_id"].as_str().unwrap();
            let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
            let saved = cursor.failure(batch).unwrap().unwrap();
            assert_eq!(saved.commit, after.commit);
            assert_eq!(saved.raw, saved.evidence.resolve(&saved.resolution_evidence_ref, TYPED).unwrap());
            assert_eq!(saved.raw, canonical(&saved.resolution_evidence).unwrap());
            assert!(cursor.failure(&"00".repeat(32)).unwrap().is_none());
            drop(cursor); drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                assert_eq!(e.reader().get().unwrap().state, after.state);
                assert_eq!(e.reader().get().unwrap().commit, after.commit);
                let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
                let replay = cursor.failure(batch).unwrap().unwrap();
                assert_eq!(replay.raw, saved.raw);
                assert_eq!(replay.resolution_evidence_ref, saved.resolution_evidence_ref);
                assert_eq!(replay.evidence.entries().count(), saved.evidence.entries().count());
                assert!(e.with_committed_attempt(&hash, |_, _| panic!("terminal callback")).is_err());
            }
        }
    }
    #[test]
    fn final_rejection_refuses_unresolved_and_successful_attempts() {
        for success in [false, true] {
            let (e, lane, s, _, _, _) = prepared_at_next(0);
            let (e, mut lane, s) = if success {
                drop(lane); drop(e);
                let (e, lane, s, _, _) = terminal_ready(0); (e, lane, s)
            } else { (e, lane, s) };
            let before = e.reader().get().unwrap().commit.clone();
            assert!(lane.reject_final_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW)).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
            assert!(lane.reject_final_with(&s, &fixture::observation(s.value()), || panic!("closed clock")).is_err());
        }
    }
    #[test]
    fn final_rejection_stale_or_clock_failure_preserves_commit() {
        for stale in [false, true] {
            let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
            lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
                |s, tx| expected_failure(s, tx), || Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap().commit.clone();
            assert!(lane.reject_final_with(&s, &fixture::observation(s.value()), ||
                if stale { Ok(fixture::NOW + 6000) } else { Err(Error::Invalid("CLOCK")) }).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
        }
    }
    #[test]
    fn receipt_persists_from_recovered_terminal_without_apply_and_replays() {
        for bps in [0, 25] {
            let (e, lane, s, inputs, home) = terminal_ready(bps);
            drop(lane); drop(e);
            let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            lane.committed_receipt_with(&s, 0, &fixture::observation(s.value()), receipt_input,
                || Ok(fixture::NOW)).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["resolution_receipts"].as_array().unwrap().len(), 1);
            for k in ["accounts", "fills", "chain_snapshot", "corrections"] {
                assert!(!before.state[k].is_null());
                assert_eq!(before.state[k], after.state[k], "{k}");
            }
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state, after.state);
                assert_eq!(e.reader().get().unwrap().commit, after.commit);
            }
        }
    }
    #[test]
    fn receipt_rejects_unresolved_attempt_before_io() {
        let (e, mut lane, s, _, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(lane.committed_receipt_with(&s, 0, &fixture::observation(s.value()),
            |_, _, _, _| panic!("unresolved IO"), || Ok(fixture::NOW)).is_err());
        assert!(lane.closed);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn receipt_forged_stale_or_io_failure_preserves_commit_and_closes() {
        for mode in 0..3 {
            let (e, mut lane, s, _, _) = terminal_ready(0);
            let before = e.reader().get().unwrap().commit.clone();
            let calls = Cell::new(0);
            assert!(lane.committed_receipt_with(&s, 0, &fixture::observation(s.value()),
                |s, t, b, tx| {
                    if mode == 2 { return Err(Error::Invalid("RPC_IO")); }
                    let (mut r, o) = receipt_input(s, t, b, tx)?;
                    if mode == 0 { r["batch_receipt_v2"] = serde_json::json!("dHg="); }
                    Ok((r, o))
                }, || { let n = calls.get(); calls.set(n+1);
                    Ok(fixture::NOW + if mode == 1 && n > 0 { 6000 } else { 0 }) }).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
        }
    }
    #[test]
    fn apply_observation_preserves_unsettled_holds_and_replays() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            let before = e.reader().get().unwrap().state.clone();
            let attempt = e.committed_attempt(&hash).unwrap();
            lane.apply_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW))
                .unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["chain_snapshot"], *s.value());
            assert_eq!(e.committed_attempt(&hash).unwrap(), attempt);
            assert_eq!(
                after.state["resolution_receipts"],
                before["resolution_receipts"]
            );
            assert_eq!(after.state["corrections"], before["corrections"]);
            for key in ["accounts", "fills", "batches"] {
                assert!(!before[key].is_null(), "missing assertion field {key}");
                assert_eq!(after.state[key], before[key], "{key}");
            }
            let state = after.state.clone();
            let commit = after.commit.clone();
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let reopened =
                    Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(reopened.reader().get().unwrap().state, state);
                assert_eq!(reopened.reader().get().unwrap().commit, commit);
            }
        }
    }
    #[test]
    fn apply_stale_closes_lane_without_commit() {
        let (e, mut lane, s, _, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.apply_with(&s, &fixture::observation(s.value()), || Ok(
                fixture::NOW + 100_000
            ))
            .is_err()
        );
        assert!(
            lane.apply_with(&s, &fixture::observation(s.value()), || panic!(
                "closed lane called clock"
            ))
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn apply_wrong_anchor_and_clock_error_do_not_commit() {
        for fail_clock in [false, true] {
            let (e, mut lane, s, _, _, _) = prepared_at_next(0);
            let before = e.reader().get().unwrap().commit.clone();
            let mut v = s.value().clone();
            v["height"] = serde_json::json!("102");
            fixture::finish(&mut v);
            let wrong = snapshot(&v, 0);
            assert!(
                lane.apply_with(&wrong, &fixture::observation(wrong.value()), || {
                    if fail_clock {
                        Err(Error::Invalid("CLOCK"))
                    } else {
                        Ok(fixture::NOW)
                    }
                })
                .is_err()
            );
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
        }
    }
    #[test]
    fn fee_profiles_prepare_persists_exact_tx_and_two_replays() {
        for bps in [0, 25] {
            let (e, s, id, inputs, home) = setup(bps);
            let signer = Sign::new();
            let mut lane = SubmitLane::new(e.clone());
            let p = lane
                .prepare_with(
                    &s,
                    &id,
                    1,
                    &signer,
                    &fixture::observation(s.value()),
                    account,
                    || Ok(fixture::NOW),
                )
                .unwrap();
            assert_eq!(signer.calls.get(), 1);
            assert!(!p.account_rpc.is_empty());
            let a = e.committed_attempt(&p.tx_hash).unwrap().unwrap();
            assert_eq!(a["state"], "PREPARED");
            assert_eq!(a["broadcast_count"], "0");
            let state = e.reader().get().unwrap().state.clone();
            assert!(
                lane.prepare_with(
                    &s,
                    &id,
                    2,
                    &signer,
                    &fixture::observation(s.value()),
                    account,
                    || Ok(fixture::NOW)
                )
                .is_err()
            );
            assert_eq!(signer.calls.get(), 1); // Existing unresolved TX cannot be replaced.
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state, state);
                assert_eq!(e.committed_attempt(&p.tx_hash).unwrap(), Some(a.clone()));
            }
        }
    }
    #[test]
    fn query_error_closes_lane_before_signing_and_retry_io() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e.clone());
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                |_, _| Err(Error::Invalid("RPC_IO")),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                |_, _| panic!("retry IO"),
                || panic!("retry clock")
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn stale_after_query_never_signs_or_commits() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e.clone());
        let before = e.reader().get().unwrap().commit.clone();
        let calls = Cell::new(0);
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                account,
                || {
                    let n = calls.get();
                    calls.set(n + 1);
                    Ok(fixture::NOW + if n == 0 { 0 } else { 6000 })
                }
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn different_account_snapshot_rejected_before_signing() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e);
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("101");
        fixture::finish(&mut v);
        let other = snapshot(&v, 0);
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                |_, owner| account(&other, owner),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
    }
    #[test]
    fn unbound_snapshot_rejected_before_account_io() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e);
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("101");
        fixture::finish(&mut v);
        let other = snapshot(&v, 0);
        assert!(
            lane.prepare_with(
                &other,
                &id,
                1,
                &sign,
                &fixture::observation(other.value()),
                |_, _| panic!("unbound IO"),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
    }
    fn prepared_at_next(
        bps: u32,
    ) -> (
        Arc<Engine>,
        SubmitLane,
        Snapshot,
        String,
        nus_exchange_contract::s3::dev_local::Inputs,
        std::path::PathBuf,
    ) {
        let (e, s, id, inputs, home) = setup(bps);
        let mut lane = SubmitLane::new(e.clone());
        let p = lane
            .prepare_with(
                &s,
                &id,
                1,
                &Sign::new(),
                &fixture::observation(s.value()),
                account,
                || Ok(fixture::NOW),
            )
            .unwrap();
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("101");
        fixture::finish(&mut v);
        let next = snapshot(&v, bps);
        e.execute(
            Command::Snapshot(canonical(&v).unwrap()),
            &[],
            &fixture::observation(&v),
            fixture::NOW,
        )
        .unwrap();
        (e, lane, next, p.tx_hash, inputs, home)
    }
    fn included(
        s: &Snapshot,
        tx: &[u8],
        code: u32,
    ) -> Result<Option<(serde_json::Value, Objects)>> {
        let (r, objects) = collect::inclusion_tests::input(s, vec![tx], serde_json::json!(code));
        collect::confirmed_in_block(s, tx, r, objects)
    }
    #[test]
    fn inclusion_persists_success_and_failure_without_releasing_assets_and_replays() {
        for bps in [0, 25] {
            for code in [0, 1019] {
                let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
                let before = e.reader().get().unwrap().state.clone();
                assert!(
                    lane.resolve_inclusion_with(
                        &s,
                        &hash,
                        &fixture::observation(s.value()),
                        |s, tx| included(s, tx, code),
                        || Ok(fixture::NOW)
                    )
                    .unwrap()
                );
                let a = e.committed_attempt(&hash).unwrap().unwrap();
                assert_eq!(
                    a["state"],
                    if code == 0 {
                        "INCLUDED_SUCCESS"
                    } else {
                        "INCLUDED_FAILURE"
                    }
                );
                assert_eq!(a["broadcast_count"], "0");
                let after = e.reader().get().unwrap().state.clone();
                // An inclusion alone does not confirm balances, correct fills,
                // apply a receipt, or replace the active batch.
                let mut before_assets = before.clone();
                let mut after_assets = after.clone();
                for key in ["attempt_refs", "last_command_seq", "stream_seq"] {
                    before_assets.as_object_mut().unwrap().remove(key);
                    after_assets.as_object_mut().unwrap().remove(key);
                }
                assert!(before_assets == after_assets, "non-attempt state changed");
                assert_ne!(before["attempt_refs"], after["attempt_refs"]);
                drop(lane);
                drop(e);
                for _ in 0..2 {
                    let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                    assert_eq!(e.committed_attempt(&hash).unwrap(), Some(a.clone()));
                    assert_eq!(e.reader().get().unwrap().state, after);
                }
            }
        }
    }
    /// Recover terminal raw and the unapplied anchor without broadcast authority.
    #[test]
    fn replay_terminal_raw_and_unapplied_observation_recover_privately() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            lane.resolve_inclusion_with(
                &s, &hash, &fixture::observation(s.value()),
                |s, tx| included(s, tx, 0), || Ok(fixture::NOW),
            ).unwrap();
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                let view = e.reader().get().unwrap();
                assert_ne!(view.state["chain_snapshot"], *s.value());
                assert_eq!(view.state["latest_observation_ref"],
                    reference(&canonical(s.value()).unwrap(), TYPED).unwrap());
                assert_eq!(e.committed_attempt(&hash).unwrap().unwrap()["state"], "INCLUDED_SUCCESS");
                let called = Cell::new(false);
                let result = e.with_committed_attempt(&hash, |_, _| called.set(true));
                assert!(matches!(result, Err(Error::Invalid("ATTEMPT_TERMINAL"))));
                assert!(!called.get());
                let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
                assert_eq!(cursor.view().unwrap().commit, view.commit);
                assert_eq!(cursor.anchors().unwrap().latest.snapshot, s);
                assert_ne!(cursor.anchors().unwrap().applied.snapshot, s);
                let page = cursor.history(None, 64).unwrap();
                assert_eq!(page.observations.last().unwrap().snapshot, s);
                assert_eq!(page.next_height, None);
                let recovered = cursor.attempt_at(0).unwrap().unwrap();
                assert_eq!(recovered.commit, view.commit);
                assert_eq!(recovered.attempt["state"], "INCLUDED_SUCCESS");
                assert_eq!(recovered.attempt["tx_hash"], hash);
                let raw = recovered.evidence.resolve(&recovered.attempt["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX).unwrap();
                assert!(!raw.is_empty());
                assert!(cursor.attempt_at(1).unwrap().is_none());
                assert_eq!(e.reader().get().unwrap().state, view.state);
                assert_eq!(e.reader().get().unwrap().commit, view.commit);
            }
        }
    }
    #[test]
    fn recovery_cursor_stale_commit_closes_without_retry() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
        lane.resolve_inclusion_with(
            &s, &hash, &fixture::observation(s.value()),
            |s, tx| included(s, tx, 0), || Ok(fixture::NOW),
        ).unwrap();
        let before = e.reader().get().unwrap();
        assert!(matches!(cursor.attempt_at(0), Err(Error::Invalid("STALE_COMMIT"))));
        assert!(matches!(cursor.history(None, 1), Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"))));
        assert!(cursor.anchors().is_err());
        assert!(cursor.view().is_err());
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
        let mut fresh = recovery::RecoveryCursor::open(e.clone()).unwrap();
        assert_eq!(fresh.attempt_at(0).unwrap().unwrap().attempt["state"], "INCLUDED_SUCCESS");
    }
    #[test]
    fn failure_cursor_stale_commit_closes_without_reconstruction() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
        lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
            |s, tx| expected_failure(s, tx), || Ok(fixture::NOW)).unwrap();
        lane.reject_final_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW)).unwrap();
        let before = e.reader().get().unwrap();
        let batch = before.state["batches"][0]["batch"]["batch_id"].as_str().unwrap();
        assert!(matches!(cursor.failure(batch), Err(Error::Invalid("STALE_COMMIT"))));
        assert!(matches!(cursor.failure(batch), Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"))));
        assert!(cursor.attempt_at(0).is_err());
        assert!(cursor.history(None, 1).is_err());
        assert!(cursor.view().is_err());
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
    }
    #[test]
    fn recovery_cursor_invalid_page_closes_and_preserves_store() {
        let (e, _, _, _, _, _) = prepared_at_next(25);
        let before = e.reader().get().unwrap();
        for limit in [0, 65, usize::MAX] {
            let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
            assert!(cursor.history(None, limit).is_err());
            assert!(cursor.attempt_at(0).is_err());
            assert!(cursor.anchors().is_err());
        }
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
        assert_eq!(e.reader().get().unwrap().state, before.state);
    }
    #[test]
    fn inclusion_not_found_leaves_attempt_and_commit_unchanged() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let a = e.committed_attempt(&hash).unwrap();
        let before = e.reader().get().unwrap().commit.clone();
        for _ in 0..2 {
            assert!(
                !lane
                    .resolve_inclusion_with(
                        &s,
                        &hash,
                        &fixture::observation(s.value()),
                        |_, _| Ok(None),
                        || Ok(fixture::NOW)
                    )
                    .unwrap()
            );
        }
        assert_eq!(e.committed_attempt(&hash).unwrap(), a);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn forged_inclusion_rejected_by_c_and_lane_stays_closed() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.resolve_inclusion_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |s, tx| {
                    let (mut v, o) = included(s, tx, 0)?.unwrap();
                    v["abci_code"] = serde_json::json!("1019");
                    Ok(Some((v, o)))
                },
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
        assert!(
            lane.resolve_inclusion_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |_, _| panic!("retry IO"),
                || panic!("retry clock")
            )
            .is_err()
        );
    }
    #[test]
    fn inclusion_stale_after_query_does_not_commit() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        let calls = Cell::new(0);
        assert!(
            lane.resolve_inclusion_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |s, tx| included(s, tx, 0),
                || {
                    let n = calls.get();
                    calls.set(n + 1);
                    Ok(fixture::NOW + if n == 0 { 0 } else { 6000 })
                }
            )
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    fn expired(
        bps: u32,
    ) -> (
        Arc<Engine>,
        SubmitLane,
        Snapshot,
        String,
        nus_exchange_contract::s3::dev_local::Inputs,
        std::path::PathBuf,
        Vec<Snapshot>,
    ) {
        let (e, lane, first, hash, inputs, home) = prepared_at_next(bps);
        let timeout =
            schema::num(&e.committed_attempt(&hash).unwrap().unwrap()["timeout_height"]).unwrap();
        let mut history = vec![first.clone()];
        let mut current = first;
        for h in current.height() + 1..=timeout + 1 {
            let mut v = current.value().clone();
            v["height"] = serde_json::json!(h.to_string());
            fixture::finish(&mut v);
            current = snapshot(&v, bps);
            e.execute(
                Command::Snapshot(canonical(&v).unwrap()),
                &[],
                &fixture::observation(&v),
                fixture::NOW,
            )
            .unwrap();
            if h <= timeout {
                history.push(current.clone());
            }
        }
        (e, lane, current, hash, inputs, home, history)
    }
    #[test]
    fn historical_inclusion_after_restart_uses_saved_height_and_replays() {
        for bps in [0, 25] {
            for code in [0, 1019] {
                let (e, lane, s, hash, inputs, home, history) = expired(bps);
                drop(lane); drop(e);
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                let before = e.reader().get().unwrap();
                let mut lane = SubmitLane::new(e.clone());
                let calls = Cell::new(0);
                assert!(lane.resolve_at_with(&s, &hash, history[0].height(),
                    &fixture::observation(s.value()), |old, tx| {
                        calls.set(calls.get()+1);
                        assert_eq!(*old, history[0]);
                        assert!(old.height() < s.height());
                        included(old, tx, code)
                    }, || Ok(fixture::NOW)).unwrap());
                assert_eq!(calls.get(), 1);
                let after = e.reader().get().unwrap();
                for k in ["balances", "batches", "chain_snapshot", "latest_observation_ref"] {
                    assert_eq!(before.state[k], after.state[k]);
                }
                let a = e.committed_attempt(&hash).unwrap().unwrap();
                assert_eq!(a["state"], if code == 0 { "INCLUDED_SUCCESS" } else { "INCLUDED_FAILURE" });
                assert_eq!(a["broadcast_count"], "0");
                drop(lane); drop(e);
                for _ in 0..2 {
                    let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                    assert_eq!(e.reader().get().unwrap().state, after.state);
                    assert_eq!(e.committed_attempt(&hash).unwrap().unwrap(), a);
                }
            }
        }
    }
    #[test]
    fn historical_miss_does_not_resolve_and_invalid_height_has_no_io() {
        let (e, mut lane, s, hash, _, _, history) = expired(0);
        let before = e.reader().get().unwrap();
        for old in &history {
            assert!(!lane.resolve_at_with(&s, &hash, old.height(),
                &fixture::observation(s.value()), |_, _| Ok(None), || Ok(fixture::NOW)).unwrap());
        }
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
        assert_eq!(e.reader().get().unwrap().state, before.state);
        for h in [history[0].height()-1, s.height(), s.height()+1] {
            let mut lane = SubmitLane::new(e.clone());
            assert!(lane.resolve_at_with(&s, &hash, h, &fixture::observation(s.value()),
                |_, _| panic!("out of range IO"), || Ok(fixture::NOW)).is_err());
            assert!(lane.resolve_at_with(&s, &hash, history[0].height(), &fixture::observation(s.value()),
                |_, _| panic!("closed IO"), || panic!("closed clock")).is_err());
        }
    }
    #[test]
    fn historical_stale_io_error_and_commit_race_close_lane() {
        for case in 0..3 {
            let (e, mut lane, s, hash, _, _, history) = expired(25);
            let before = e.reader().get().unwrap();
            let clocks = Cell::new(0);
            assert!(lane.resolve_at_with(&s, &hash, history[0].height(),
                &fixture::observation(s.value()), |old, tx| {
                    if case == 0 { return Err(Error::Invalid("IO")); }
                    if case == 2 {
                        let mut other = SubmitLane::new(e.clone());
                        other.resolve_at_with(&s, &hash, old.height(), &fixture::observation(s.value()),
                            |old, tx| included(old, tx, 0), || Ok(fixture::NOW))?;
                    }
                    included(old, tx, 0)
                }, || { let n = clocks.get(); clocks.set(n+1);
                    Ok(fixture::NOW + if case == 1 && n > 0 {6000} else {0}) }).is_err());
            if case < 2 { assert_eq!(e.reader().get().unwrap().commit, before.commit); }
            else { assert_ne!(e.reader().get().unwrap().commit, before.commit); }
            let after = e.reader().get().unwrap();
            assert!(lane.resolve_at_with(&s, &hash, history[0].height(), &fixture::observation(s.value()),
                |_, _| panic!("closed IO"), || panic!("closed clock")).is_err());
            assert_eq!(e.reader().get().unwrap().commit, after.commit);
        }
    }
    #[test]
    fn inclusion_scan_one_height_per_tick_restart_and_no_absence_promotion() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home, history) = expired(bps);
            let before = e.reader().get().unwrap();
            let o = fixture::observation(s.value());
            for old in &history {
                assert_eq!(lane.scan_inclusion_with(&s, &hash, &o, |actual, _| {
                    assert_eq!(actual, old); Ok(None)
                }, || Ok(fixture::NOW)).unwrap(), InclusionProgress::Missing(old.height()));
            }
            assert_eq!(lane.scan_inclusion_with(&s, &hash, &o,
                |_, _| panic!("exhausted IO"), || Ok(fixture::NOW)).unwrap(),
                InclusionProgress::WindowScanned);
            assert_eq!(e.reader().get().unwrap().commit, before.commit);
            assert_eq!(e.reader().get().unwrap().state, before.state);
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                let mut lane = SubmitLane::new(e.clone());
                assert_eq!(lane.scan_inclusion_with(&s, &hash, &o, |actual, _| {
                    assert_eq!(actual, &history[0]); Ok(None)
                }, || Ok(fixture::NOW)).unwrap(), InclusionProgress::Missing(history[0].height()));
                assert_eq!(e.reader().get().unwrap().state, before.state);
            }
        }
    }
    #[test]
    fn inclusion_scan_waits_for_height_then_resolves_and_refuses_terminal() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, _, _) = prepared_at_next(bps);
            let o = fixture::observation(s.value());
            assert_eq!(lane.scan_inclusion_with(&s, &hash, &o, |_, _| Ok(None),
                || Ok(fixture::NOW)).unwrap(), InclusionProgress::Missing(s.height()));
            assert_eq!(lane.scan_inclusion_with(&s, &hash, &o, |_, _| panic!("future IO"),
                || Ok(fixture::NOW)).unwrap(), InclusionProgress::Waiting);
            let mut v = s.value().clone(); v["height"] = serde_json::json!((s.height()+1).to_string());
            fixture::finish(&mut v);
            let s = snapshot(&v, bps); let o = fixture::observation(&v);
            e.execute(Command::Snapshot(canonical(&v).unwrap()), &[], &o, fixture::NOW).unwrap();
            assert_eq!(lane.scan_inclusion_with(&s, &hash, &o, |old, tx| included(old, tx, 1019),
                || Ok(fixture::NOW)).unwrap(), InclusionProgress::Included(s.height()));
            assert!(lane.scan_inclusion_with(&s, &hash, &o, |_, _| panic!("terminal IO"),
                || Ok(fixture::NOW)).is_err());
            assert!(lane.closed);
        }
    }
    #[test]
    fn inclusion_scan_error_or_panic_closes_without_advancing() {
        for case in 0..3 {
            let (e, mut lane, s, hash, _, _, _) = expired(0);
            let before = e.reader().get().unwrap();
            let o = fixture::observation(s.value());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lane.scan_inclusion_with(&s, &hash, &o, |_, _| {
                    if case == 0 { panic!("injected"); }
                    Err(Error::Invalid("IO"))
                }, || Ok(fixture::NOW + if case == 2 {6000} else {0}))
            }));
            assert!(result.is_err() || result.unwrap().is_err());
            assert!(lane.closed); assert!(lane.inclusion_next.is_none());
            assert!(lane.scan_inclusion_with(&s, &hash, &o, |_, _| panic!("closed IO"),
                || panic!("closed clock")).is_err());
            assert_eq!(e.reader().get().unwrap().commit, before.commit);
        }
    }
    fn absent(
        s: &Snapshot,
        a: &serde_json::Value,
        history: &[Snapshot],
    ) -> Result<(serde_json::Value, Objects)> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let b = serde_json::json!({"context":s.context(),"observed_height":s.height().to_string(),
            "snapshot_id":s.id(),"requested_seq":a["batch"]["batch_seq"],
            "last_seq":s.value()["last_batch_seq"],"last_hash":s.value()["last_batch_hash"],
            "status":"NOT_FOUND_AT_HEIGHT","receipt":null});
        let raw = serde_json::to_vec(
            &serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"response":{"code":0,
            "height":s.height().to_string(),"value":STANDARD.encode(canonical(&b)?)}}}),
        )
        .unwrap();
        let auth = account(s, &schema::bytes(&a["operator"])?)?;
        collect::collect_absence(
            s,
            &history.iter().collect::<Vec<_>>(),
            a,
            Some(&auth),
            |_| Ok(raw),
            |s| {
                Ok(collect::inclusion_tests::input(
                    s,
                    vec![],
                    serde_json::json!(0),
                ))
            },
        )
    }
    #[test]
    fn timeout_history_recovers_exact_window_twice_after_restart() {
        for bps in [0, 25] {
            let (e, lane, s, hash, inputs, home, history) = expired(bps);
            let before = e.reader().get().unwrap();
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
                let page = cursor.timeout_history(&hash).unwrap();
                assert_eq!(page.latest.snapshot, s);
                assert_eq!(page.observations.len(), 8);
                assert_eq!(page.observations.iter().map(|r| r.snapshot.clone()).collect::<Vec<_>>(), history);
                for row in &page.observations {
                    assert_eq!(s.decode_related(&row.raw).unwrap(), row.snapshot);
                }
                assert_eq!(e.reader().get().unwrap().commit, before.commit);
                assert_eq!(e.reader().get().unwrap().state, before.state);
            }
        }
    }
    #[test]
    fn timeout_history_early_unknown_or_terminal_closes_cursor() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        for h in [&hash, &"f".repeat(64)] {
            let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
            assert!(cursor.timeout_history(h).is_err());
            assert!(cursor.history(None, 1).is_err());
            assert!(cursor.view().is_err());
        }
        assert_eq!(e.reader().get().unwrap().commit, before);
        lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
            |s, tx| included(s, tx, 0), || Ok(fixture::NOW)).unwrap();
        let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
        assert!(matches!(cursor.timeout_history(&hash), Err(Error::Invalid("ATTEMPT_TERMINAL"))));
        assert!(cursor.anchors().is_err());
    }
    #[test]
    fn timeout_history_commit_race_never_returns_mixed_history() {
        let (e, mut lane, s, hash, _, _, history) = expired(25);
        let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
        lane.resolve_absence_with(&s, &hash, &fixture::observation(s.value()),
            |s, a| absent(s, a, &history), || Ok(fixture::NOW)).unwrap();
        let before = e.reader().get().unwrap();
        assert!(matches!(cursor.timeout_history(&hash), Err(Error::Invalid("STALE_COMMIT"))));
        assert!(cursor.attempt_at(0).is_err());
        assert!(cursor.view().is_err());
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
        assert_eq!(e.reader().get().unwrap().state, before.state);
    }
    #[test]
    fn absence_persists_without_asset_release_and_replays_twice() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home, history) = expired(bps);
            let before = e.reader().get().unwrap().state.clone();
            lane.resolve_absence_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |s, a| absent(s, a, &history),
                || Ok(fixture::NOW),
            )
            .unwrap();
            let a = e.committed_attempt(&hash).unwrap().unwrap();
            assert_eq!(a["state"], "EXPIRED_ABSENT_PROVEN");
            let after = e.reader().get().unwrap().state.clone();
            let mut left = before;
            let mut right = after.clone();
            for k in ["attempt_refs", "last_command_seq", "stream_seq"] {
                left.as_object_mut().unwrap().remove(k);
                right.as_object_mut().unwrap().remove(k);
            }
            assert!(left == right, "assets changed on absence");
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.committed_attempt(&hash).unwrap(), Some(a.clone()));
                assert_eq!(e.reader().get().unwrap().state, after);
            }
        }
    }
    #[test]
    fn absence_incomplete_forged_stale_or_io_error_never_commits() {
        for mode in 0..4 {
            let (e, mut lane, s, hash, _, _, history) = expired(0);
            let before = e.reader().get().unwrap().commit.clone();
            let calls = Cell::new(0);
            assert!(
                lane.resolve_absence_with(
                    &s,
                    &hash,
                    &fixture::observation(s.value()),
                    |s, a| {
                        if mode == 0 {
                            return absent(s, a, &history[..7]);
                        }
                        if mode == 3 {
                            return Err(Error::Invalid("RPC_IO"));
                        }
                        let (mut p, o) = absent(s, a, &history)?;
                        if mode == 1 {
                            p["tx_hash"] = serde_json::json!("00".repeat(32));
                        }
                        Ok((p, o))
                    },
                    || {
                        let n = calls.get();
                        calls.set(n + 1);
                        Ok(fixture::NOW + if mode == 2 && n > 0 { 6000 } else { 0 })
                    }
                )
                .is_err()
            );
            assert_eq!(e.reader().get().unwrap().commit, before);
            assert!(
                lane.resolve_absence_with(
                    &s,
                    &hash,
                    &fixture::observation(s.value()),
                    |_, _| panic!("retry IO"),
                    || panic!("retry clock")
                )
                .is_err()
            );
        }
    }
    #[test]
    fn absence_before_timeout_rejected_before_io() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.resolve_absence_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |_, _| panic!("early IO"),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
}

//! Dormant L-D component. Service startup requires the separately reviewed L-R pin.
mod rpc;
use super::{
    dev_local::{Command, Engine, Error, Result},
    schema,
    snapshot::Observation,
};
pub use rpc::LoopbackRpc;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// One serialized submission lane; C remains the authoritative writer/effect gate.
/// The durable UNKNOWN/count transition is a conservative broadcast intent:
/// a crash before the socket write still consumes this transmission budget.
pub struct Worker {
    engine: Arc<Engine>,
    lane: Mutex<()>,
}
impl Worker {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            lane: Mutex::new(()),
        }
    }
    /// Runtime entry for a new SETTLE envelope. Check all unresolved attempts
    /// before invoking the private signer; caller supplies B's fresh account query.
    pub fn prepare_settle(
        &self,
        snapshot: &super::snapshot::Snapshot,
        batch_id: &str,
        attempt_no: u64,
        account_number: u64,
        account_sequence: u64,
        signer: &impl chain::OperatorSigner,
        o: &Observation,
        now: u64,
    ) -> Result<String> {
        let _lane = self
            .lane
            .lock()
            .map_err(|_| Error::Recovery("WORKER_POISONED"))?;
        let view = self.engine.reader().get()?;
        if view.gate == "RECOVERY_REQUIRED" {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        let latest = &view.state["latest_observation_ref"];
        let matches_latest = if latest.is_null() {
            view.state["chain_snapshot"] == *snapshot.value()
        } else {
            *latest
                == super::evidence::reference(
                    &super::journal::canonical(snapshot.value())?,
                    super::evidence::TYPED,
                )?
        };
        if !matches_latest || view.state["context"] != *snapshot.context() {
            return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
        }
        snapshot.freshness(o, now)?;
        let batch = view.state["batches"]
            .as_array()
            .ok_or("BATCH_NOT_FOUND")?
            .iter()
            .find(|b| b["batch"]["batch_id"] == batch_id)
            .ok_or("BATCH_NOT_FOUND")?;
        if !["SEALED", "SUBMISSION_UNKNOWN"].contains(&batch["state"].as_str().unwrap_or("")) {
            return Err(Error::Invalid("BATCH_STATE"));
        }
        let mut prior_settles = 0u64;
        for hash in batch["attempt_hashes"].as_array().ok_or("BATCH_STATE")? {
            let a = self
                .engine
                .committed_attempt(hash.as_str().ok_or("BATCH_STATE")?)?
                .ok_or(Error::Recovery("ATTEMPT_NOT_FOUND"))?;
            if a["kind"] == "SETTLE" {
                prior_settles += 1;
            }
            if ["PREPARED", "SUBMISSION_UNKNOWN"].contains(&a["state"].as_str().unwrap_or("")) {
                return Err(Error::Invalid("ATTEMPT_UNRESOLVED"));
            }
        }
        if prior_settles >= 3 || attempt_no != prior_settles + 1 {
            return Err(Error::Invalid("RETRY_BUDGET_EXHAUSTED"));
        }
        let raw = chain::sealed_batch(&view.state, batch_id)?;
        let (a, tx) = chain::settle_attempt(
            snapshot,
            &raw,
            attempt_no,
            account_number,
            account_sequence,
            signer,
        )?;
        let hash = a["tx_hash"].as_str().ok_or("INVALID_ENVELOPE")?.to_owned();
        self.engine.execute(
            Command::Attempt(a),
            &[(tx, super::evidence::TX.into())],
            o,
            now,
        )?;
        Ok(hash)
    }
    /// Prepare CLOSE from C's original committed failure graph, never from a
    /// newly selected failure or a browser payload. `expected` pins the view
    /// used by the trusted adapter for its same-height B Account query. Account
    /// number/sequence must come from that query (as for prepare_settle).
    /// This method signs and commits only; use broadcast for durable intent/IO.
    pub fn prepare_close(
        &self,
        expected: &super::journal::Commit,
        snapshot: &super::snapshot::Snapshot,
        batch_id: &str,
        attempt_no: u64,
        account_number: u64,
        account_sequence: u64,
        signer: &impl chain::OperatorSigner,
        o: &Observation,
        now: u64,
    ) -> Result<String> {
        let _lane = self
            .lane
            .lock()
            .map_err(|_| Error::Recovery("WORKER_POISONED"))?;
        let view = self.engine.reader().get()?;
        if view.gate == "RECOVERY_REQUIRED" {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        if view.commit != *expected {
            return Err(Error::Invalid("STALE_COMMIT"));
        }
        let latest = &view.state["latest_observation_ref"];
        let matches_latest = if latest.is_null() {
            view.state["chain_snapshot"] == *snapshot.value()
        } else {
            *latest
                == super::evidence::reference(
                    &super::journal::canonical(snapshot.value())?,
                    super::evidence::TYPED,
                )?
        };
        if !matches_latest || view.state["context"] != *snapshot.context() {
            return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
        }
        snapshot.freshness(o, now)?;
        let batch = view.state["batches"]
            .as_array()
            .ok_or("BATCH_NOT_FOUND")?
            .iter()
            .find(|b| b["batch"]["batch_id"] == batch_id)
            .ok_or("BATCH_NOT_FOUND")?;
        if !["REJECTED_FINAL", "CLOSING"].contains(&batch["state"].as_str().unwrap_or("")) {
            return Err(Error::Invalid("BATCH_STATE"));
        }
        let mut prior_closes = 0u64;
        for hash in batch["attempt_hashes"].as_array().ok_or("BATCH_STATE")? {
            let recovered = self
                .engine
                .trusted_recovery_attempt(expected, hash.as_str().ok_or("BATCH_STATE")?)?
                .ok_or(Error::Recovery("ATTEMPT_NOT_FOUND"))?;
            let a = &recovered.attempt;
            if a["batch"] != batch["batch"] || a["context"] != *snapshot.context() {
                return Err(Error::Recovery("RECOVERY_ATTEMPT_MISMATCH"));
            }
            if ["PREPARED", "SUBMISSION_UNKNOWN"].contains(&a["state"].as_str().unwrap_or("")) {
                return Err(Error::Invalid("ATTEMPT_UNRESOLVED"));
            }
            if a["kind"] == "CLOSE" {
                prior_closes += 1;
            }
        }
        if prior_closes >= 2 || attempt_no != prior_closes + 1 {
            return Err(Error::Invalid("RETRY_BUDGET_EXHAUSTED"));
        }
        let raw = chain::sealed_batch(&view.state, batch_id)?;
        // The final pinned store read also checks original root/descriptor/raw
        // integrity; missing originals close C before signer access.
        let failure = self
            .engine
            .trusted_recovery_failure(expected, batch_id)?
            .ok_or(Error::Invalid("FAILURE_EVIDENCE_REQUIRED"))?;
        let (a, tx) = chain::close_attempt(
            snapshot,
            &raw,
            attempt_no,
            account_number,
            account_sequence,
            signer,
            &failure.resolution_evidence,
        )?;
        // Signer is outside C's writer lock. Discard bytes if it raced a commit;
        // C still revalidates authoritative state/policy under its writer lock.
        if self.engine.reader().get()?.commit != *expected {
            return Err(Error::Invalid("STALE_COMMIT"));
        }
        let hash = a["tx_hash"].as_str().ok_or("INVALID_ENVELOPE")?.to_owned();
        self.engine
            .execute(
                Command::Attempt(a),
                &[(tx, super::evidence::TX.into())],
                o,
                now,
            )?
            .ok_or(Error::Recovery("ATTEMPT_NOT_COMMITTED"))?;
        Ok(hash)
    }
    pub fn prepare(
        &self,
        attempt: Value,
        evidence: &[(Vec<u8>, String)],
        o: &Observation,
        now: u64,
    ) -> Result<()> {
        self.engine
            .execute(Command::Attempt(attempt), evidence, o, now)?;
        Ok(())
    }
    fn broadcast_inner<T>(
        &self,
        hash: &str,
        o: &Observation,
        now: u64,
        effect: impl FnOnce(&Value, &[u8]) -> T,
    ) -> Result<T> {
        let _lane = self
            .lane
            .lock()
            .map_err(|_| Error::Recovery("WORKER_POISONED"))?;
        let mut intent = self
            .engine
            .committed_attempt(hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        if !["PREPARED", "SUBMISSION_UNKNOWN"].contains(&intent["state"].as_str().unwrap_or("")) {
            return Err(Error::Invalid("ATTEMPT_TERMINAL"));
        }
        let count = schema::num(&intent["broadcast_count"])?;
        if count >= 3 {
            return Err(Error::Invalid("RETRY_BUDGET_EXHAUSTED"));
        }
        // The inherited profile requires 0/1000/2000ms. Wait the full delay
        // even after restart: no in-memory timestamp can reset the durable count.
        // This is outside C's writer lock; the exact intent is checked again below.
        std::thread::sleep(std::time::Duration::from_millis(count * 1000));
        intent["state"] = json!("SUBMISSION_UNKNOWN");
        intent["broadcast_count"] = json!((count + 1).to_string());
        // WAL/objects/marker fsync and publication must finish before entering callback.
        self.engine
            .execute(Command::Resolve(intent.clone()), &[], o, now)?
            .ok_or(Error::Recovery("INTENT_NOT_COMMITTED"))?;
        self.engine
            .with_committed_attempt(hash, |stored, raw| {
                if stored != &intent {
                    return Err(Error::Recovery("INTENT_CHANGED"));
                }
                Ok(effect(stored, raw))
            })?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?
    }
    /// CheckTx, timeout, response loss and NOT_FOUND never resolve an attempt.
    /// No automatic replacement TX, new batch, release or correction is performed.
    pub fn broadcast(
        &self,
        hash: &str,
        rpc: &LoopbackRpc,
        o: &Observation,
        now: u64,
    ) -> Result<Value> {
        self.broadcast_inner(hash, o, now, |_, raw| {
            let _ = rpc.broadcast(raw);
        })?;
        Ok(json!({"tx_hash":hash,"state":"SUBMISSION_UNKNOWN","durable_ack":false}))
    }
    /// Trusted chain adapter only: C revalidates exact raw evidence and same-H proof.
    /// Never route these commands from browser bodies.
    pub fn reconcile(
        &self,
        command: Command,
        evidence: &[(Vec<u8>, String)],
        o: &Observation,
        now: u64,
    ) -> Result<Option<Value>> {
        if !matches!(
            command,
            Command::Snapshot(_)
                | Command::Resolve(_)
                | Command::Receipt(_)
                | Command::RejectFinal
                | Command::Apply
                | Command::Seal(_)
        ) {
            return Err(Error::Invalid("WORKER_COMMAND"));
        }
        self.engine.execute(command, evidence, o, now)
    }
    #[cfg(feature = "fault-injection")]
    pub fn test_broadcast<T>(
        &self,
        hash: &str,
        o: &Observation,
        now: u64,
        effect: impl FnOnce(&Value, &[u8]) -> T,
    ) -> Result<T> {
        self.broadcast_inner(hash, o, now, effect)
    }
}
mod rest;
pub use rest::{Options, Request, Rest};
pub mod chain;

//! Serialized service boundary. Live RPC health is deliberately not replayed as
//! proof of current freshness: every process starts closed until re-observation.
//! Transport authentication and RPC fetching belong to the outer adapter.
use super::{
    journal::{Error, Result},
    recovery::SignedRecovery,
    sequencer::Candidate,
    snapshot::{Observation, Snapshot},
};
use serde_json::Value;

pub struct Service {
    engine: SignedRecovery,
    observation: Option<Observation>,
    required_height: u64,
    failure: Option<(&'static str, &'static str)>,
}
impl Service {
    pub fn new(engine: SignedRecovery) -> Self {
        let required_height = engine.state().snapshot().height();
        Self {
            required_height,
            engine,
            observation: None,
            failure: None,
        }
    }
    /// No mutable recovery handle escapes this gate.
    pub fn state(&self) -> &Candidate {
        self.engine.state()
    }
    /// Mode/reason are a live transport projection, not a rewrite of a receipt
    /// or persisted EngineState hash. The API must expose this projection.
    pub fn admission(&self, now: u64) -> (&'static str, &'static str) {
        if self.engine.recovery_required() {
            return ("RECOVERY_REQUIRED", "POISONED_SESSION");
        }
        if let Some(failure) = self.failure {
            return failure;
        }
        let Some(observation) = &self.observation else {
            return ("CATCHING_UP", "OBSERVATION_REQUIRED");
        };
        match self.state().snapshot().freshness(observation, now) {
            Ok(()) => ("OPEN", "OK"),
            Err("CATCHING_UP") => ("CATCHING_UP", "CATCHING_UP"),
            Err(reason) => ("STALE", reason),
        }
    }
    pub fn observation(&self) -> Option<&Observation> {
        self.observation.as_ref()
    }
    pub fn rpc_failed(&mut self) {
        if !matches!(self.failure, Some(("RECOVERY_REQUIRED", _))) {
            self.failure = Some(("STALE", "RPC_UNAVAILABLE"));
        }
    }
    /// Accept only a manifest-validated snapshot from the trusted adapter.
    /// Gaps can catch up; conflicting history requires explicit recovery.
    pub fn observe(
        &mut self,
        snapshot: Snapshot,
        observation: Observation,
        now: u64,
        maximum_correction_payload_bytes: usize,
    ) -> Result<Option<Value>> {
        if matches!(self.failure, Some(("RECOVERY_REQUIRED", _))) {
            return Err(Error::RecoveryRequired("OBSERVATION_CONFLICT"));
        }
        if observation.snapshot_id != snapshot.id()
            || observation.cursor_height != snapshot.height()
        {
            self.failure = Some(("RECOVERY_REQUIRED", "SNAPSHOT_CONFLICT"));
            return Err(Error::InvalidRecord("SNAPSHOT_CONFLICT"));
        }
        self.required_height = self.required_height.max(snapshot.height());
        match self.engine.advance(
            snapshot,
            &observation,
            now,
            maximum_correction_payload_bytes,
        ) {
            Ok(result) => {
                self.observation = Some(observation);
                self.failure = (self.state().snapshot().height() < self.required_height)
                    .then_some(("CATCHING_UP", "CATCHING_UP"));
                Ok(result)
            }
            Err(error) => {
                self.failure = Some(match &error {
                    Error::InvalidRecord("CATCHING_UP") => ("CATCHING_UP", "CATCHING_UP"),
                    Error::ResourceLimit => ("RECOVERY_REQUIRED", "RESOURCE_LIMIT"),
                    Error::InvalidRecord(reason) => ("RECOVERY_REQUIRED", reason),
                    _ => ("RECOVERY_REQUIRED", "JOURNAL_FAILURE"),
                });
                Err(error)
            }
        }
    }
    /// The owner is supplied by an authenticated adapter. Only ORDER and abort
    /// need OPEN; cancellation and withdraw freezing cannot create new matching.
    /// Original authenticated retries retain their receipt even while closed.
    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &mut self,
        kind: &str,
        raw: &[u8],
        signature: &[u8],
        owner: &str,
        now: u64,
        maximum_correction_payload_bytes: usize,
    ) -> Result<Value> {
        let (mode, reason) = self.admission(now);
        let error = if mode == "RECOVERY_REQUIRED"
            || (mode != "OPEN" && matches!(kind, "ORDER" | "WITHDRAW_ABORT"))
        {
            Some(reason)
        } else {
            None
        };
        // This placeholder can never authorize new matching; the closed gate
        // rejects it. It allows safe cancellation and original receipt lookup.
        let missing = Observation {
            snapshot_id: self.state().snapshot().id().into(),
            cursor_height: self.state().snapshot().height(),
            received_at: now,
            query_latency_ms: 0,
            catching_up: true,
        };
        let observation = self.observation.as_ref().unwrap_or(&missing);
        self.engine.submit_admitted(
            kind,
            raw,
            signature,
            owner,
            observation,
            now,
            maximum_correction_payload_bytes,
            error,
        )
    }
    pub fn receipt(&self, owner: &str, sequence: u64) -> Option<&Value> {
        self.engine.receipt(owner, sequence)
    }
}

//! Serialized service boundary. Live RPC health is deliberately not replayed as
//! proof of current freshness: every process starts closed until re-observation.
//! Transport authentication and RPC fetching belong to the outer adapter.
use super::{
    journal::{Error, Result},
    recovery::SignedRecovery,
    sequencer::Candidate,
    snapshot::{Observation, Snapshot},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
    /// Public book from exactly one committed engine sequence. Runtime health
    /// is deliberately separate: failed observations cannot change this hash.
    /// No owner, order identifier or signed evidence is exposed.
    pub fn book(&self) -> Result<Value> {
        let state = self.state();
        let mut bids = BTreeMap::<u64, (u64, u32)>::new();
        let mut asks = BTreeMap::<u64, (u64, u32)>::new();
        for order in state.orders().values() {
            if !matches!(order.status.as_str(), "OPEN" | "PARTIALLY_FILLED")
                || order.live.remaining == 0
            {
                continue;
            }
            let levels = match order.live.side {
                super::ledger::Side::Buy => &mut bids,
                super::ledger::Side::Sell => &mut asks,
            };
            let level = levels.entry(order.live.price).or_default();
            level.0 = level
                .0
                .checked_add(order.live.remaining)
                .ok_or(Error::ResourceLimit)?;
            level.1 = level.1.checked_add(1).ok_or(Error::ResourceLimit)?;
        }
        if bids.len() > 200 || asks.len() > 200 {
            return Err(Error::ResourceLimit);
        }
        let level = |(price, (qty, count)): (u64, (u64, u32))| {
            json!({"price_ticks":price.to_string(), "qty_lots":qty.to_string(),
                "order_count":count.to_string()})
        };
        let mut book = json!({
            "context":state.snapshot().value()["body"]["context"],
            "stream_seq":state.sequence().to_string(),
            "revision":state.sequence().to_string(),
            "snapshot_id":state.snapshot().id(),
            "observed_height":state.snapshot().height().to_string(),
            "bids":bids.into_iter().rev().map(level).collect::<Vec<_>>(),
            "asks":asks.into_iter().map(level).collect::<Vec<_>>()
        });
        book["content_hash"] = json!(super::journal::sha256(&crate::codec::frame(
            "NUS/S2/BOOK/V1",
            &super::journal::canonical(&book)?
        )));
        Ok(book)
    }
    /// Contract Observation projection. Before live re-observation, zero receipt
    /// time and maximum ages denote unknown freshness, never a successful RPC.
    /// A failed RPC retains the last success metadata but closes `fresh`.
    pub fn observation_view(&self, now: u64) -> Value {
        let snapshot = self.state().snapshot();
        let block_time = snapshot.value()["body"]["block_time_unix_ms"]
            .as_str()
            .expect("validated snapshot timestamp")
            .parse::<u64>()
            .expect("validated snapshot timestamp");
        let (received_at, cursor, latency, success_age, catching_up) = match &self.observation {
            Some(observation) => (
                observation.received_at,
                observation.cursor_height,
                observation.query_latency_ms,
                now.checked_sub(observation.received_at).unwrap_or(u64::MAX),
                observation.catching_up || snapshot.height() < self.required_height,
            ),
            None => (0, snapshot.height(), 0, u64::MAX, true),
        };
        json!({
            "snapshot_id": snapshot.id(),
            "observed_height": snapshot.height().to_string(),
            "cursor_height": cursor.to_string(),
            "received_at_unix_ms": received_at.to_string(),
            "block_age_ms": now.saturating_sub(block_time).to_string(),
            "query_latency_ms": latency.to_string(),
            "last_success_age_ms": success_age.to_string(),
            "catching_up": catching_up,
            "fresh": self.admission(now).0 == "OPEN"
        })
    }
    /// Owner must come from a verified session, never a query parameter.
    /// Returns private LedgerView components; live Status is attached separately.
    pub fn private_page(&self, owner: &str, cursor: Option<&str>) -> Result<Value> {
        super::private_view::page(self.state(), owner, cursor, 200, 1000)
            .map_err(Error::InvalidRecord)
    }
    /// Status revision identifies the committed engine state. Observation ages
    /// and the runtime gate are live metadata and do not append journal commands.
    /// Callers must evaluate freshness on every response, even at unchanged seq.
    pub fn status(&self, now: u64) -> Value {
        let (mode, reason) = self.admission(now);
        json!({
            "context":self.state().snapshot().value()["body"]["context"],
            "stream_seq":self.state().sequence().to_string(),
            "revision":self.state().sequence().to_string(),
            "mode":mode, "reason":reason,
            "observation":self.observation_view(now),
            "durability":"LOCAL_FSYNC", "replicated":false,
            "settlement_submission_enabled":false
        })
    }
    /// Complete owner view from one immutable committed state and one supplied
    /// clock instant. Authentication is the outer adapter's responsibility.
    pub fn ledger_view(&self, owner: &str, cursor: Option<&str>, now: u64) -> Result<Value> {
        let mut page = self.private_page(owner, cursor)?;
        let mut status = self.status(now);
        // Global recovery/staleness takes priority over account-local freezing.
        if status["mode"] == "OPEN" && self.state().is_frozen(owner) {
            status["mode"] = json!("WITHDRAW_FROZEN");
            status["reason"] = json!("WITHDRAW_FROZEN");
        }
        page["status"] = status;
        Ok(page)
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

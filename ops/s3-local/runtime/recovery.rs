//! Private, commit-pinned recovery cursor. No network, freshness or effect permit.
use nus_exchange_contract::s3::dev_local::{
    Engine, Error, RecoveryAttempt, RecoveryFailure, RecoveryHistory, Result, View,
};
use std::sync::Arc;

/// A cursor retains only one immutable view and one bounded page. Consumers
/// discard the entire cursor on error; no mixed-revision retry is performed.
pub struct RecoveryCursor {
    engine: Arc<Engine>,
    view: Arc<View>,
    anchors: RecoveryHistory,
    closed: bool,
}
impl RecoveryCursor {
    pub fn open(engine: Arc<Engine>) -> Result<Self> {
        let view = engine.reader().get()?;
        // One row suffices to recover both anchors; full history is paged later.
        let anchors = engine.trusted_recovery_history(&view.commit, None, 1)?;
        if anchors.commit != view.commit
            || *anchors.applied.snapshot.value() != view.state["chain_snapshot"]
        {
            return Err(Error::Recovery("RECOVERY_ANCHOR"));
        }
        Ok(Self {
            engine,
            view,
            anchors,
            closed: false,
        })
    }
    /// No Observation is produced: persisted block time is not a fresh RPC read.
    pub fn anchors(&self) -> Result<&RecoveryHistory> {
        if self.closed {
            return Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"));
        }
        Ok(&self.anchors)
    }
    pub fn view(&self) -> Result<&View> {
        if self.closed {
            return Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"));
        }
        Ok(&self.view)
    }
    pub fn history(&mut self, from: Option<u64>, limit: usize) -> Result<RecoveryHistory> {
        if self.closed {
            return Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"));
        }
        self.closed = true;
        let page = self
            .engine
            .trusted_recovery_history(&self.view.commit, from, limit)?;
        self.closed = false;
        Ok(page)
    }
    /// Recover exactly the persisted eight-height window for one unresolved TX.
    /// This is history, not absence evidence: the caller still needs raw RPC
    /// block/results, Account and batch lookup and C's proof validation.
    pub fn timeout_history(&mut self, hash: &str) -> Result<RecoveryHistory> {
        if self.closed {
            return Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"));
        }
        self.closed = true;
        let saved = self.engine.trusted_recovery_attempt(&self.view.commit, hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        let a = &saved.attempt;
        if !matches!(a["state"].as_str(), Some("PREPARED" | "SUBMISSION_UNKNOWN")) {
            return Err(Error::Invalid("ATTEMPT_TERMINAL"));
        }
        let first = nus_exchange_contract::s3::schema::num(&a["first_possible_height"])?;
        let last = nus_exchange_contract::s3::schema::num(&a["timeout_height"])?;
        if last.checked_sub(first).and_then(|n| n.checked_add(1)) != Some(8)
            || self.anchors.latest.snapshot.height() <= last {
            return Err(Error::Invalid("TIMEOUT_HISTORY_RANGE"));
        }
        let page = self.engine.trusted_recovery_history(&self.view.commit, Some(first), 8)?;
        if page.commit != saved.commit || page.observations.len() != 8
            || page.latest.snapshot != self.anchors.latest.snapshot {
            return Err(Error::Recovery("TIMEOUT_HISTORY_INCOMPLETE"));
        }
        for (i, row) in page.observations.iter().enumerate() {
            if row.snapshot.height() != first + i as u64
                || row.snapshot.context() != &a["context"] {
                return Err(Error::Recovery("TIMEOUT_HISTORY_CONFLICT"));
            }
        }
        self.closed = false;
        Ok(page)
    }
    /// Exact persisted failure bytes only; no reconstruction or broadcast permit.
    pub fn failure(&mut self, batch_id: &str) -> Result<Option<RecoveryFailure>> {
        if self.closed {
            return Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"));
        }
        self.closed = true;
        let saved = self.engine.trusted_recovery_failure(&self.view.commit, batch_id)?;
        self.closed = false;
        Ok(saved)
    }
    pub fn attempt_at(&mut self, index: usize) -> Result<Option<RecoveryAttempt>> {
        if self.closed {
            return Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"));
        }
        self.closed = true;
        let attempt = self
            .engine
            .trusted_recovery_attempt_at(&self.view.commit, index)?;
        self.closed = false;
        Ok(attempt)
    }
}

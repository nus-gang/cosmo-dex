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

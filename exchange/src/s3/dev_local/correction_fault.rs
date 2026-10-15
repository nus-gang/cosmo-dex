//! F14 exists only in a dev-local-demo + fault-injection build. No persisted
//! fields, environment switches, economic decisions or recovery-time hooks.
use super::{Error, Result, storage_error};
use std::{cell::RefCell, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectionPhase {
    Prepare,
    SemanticReplay,
}

/// Owned copies of the already-computed closure, before correction mutations.
/// This is fault-test evidence, never an economic input or permission to publish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorrectionBoundary {
    pub phase: CorrectionPhase,
    pub root_fill_ids: Vec<String>,
    pub corrected_fill_ids: Vec<String>,
    pub surviving_fill_ids: Vec<String>,
}
pub type CorrectionHook = Arc<dyn Fn(&CorrectionBoundary) -> Result<()> + Send + Sync>;
struct Active {
    hook: CorrectionHook,
    phase: CorrectionPhase,
    error: Option<Error>,
}
thread_local! { static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) }; }

/// Scope only the two real execute calculations. In particular, open/replay,
/// readiness probes and direct Candidate calls never inherit a stored hook.
pub(super) fn scoped<T>(
    hook: &Option<CorrectionHook>,
    phase: CorrectionPhase,
    f: impl FnOnce() -> crate::Result<T>,
) -> Result<T> {
    struct Reset(Option<Active>);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with_borrow_mut(|active| *active = self.0.take());
        }
    }
    let _reset = Reset(ACTIVE.replace(hook.as_ref().map(|hook| Active {
        hook: hook.clone(),
        phase,
        error: None,
    })));
    let result = f();
    // Preserve the original IO error and distinguish injection from ordinary
    // rejected input. An injected Invalid must still close the writer lane.
    if let Some(error) = ACTIVE.with_borrow_mut(|a| a.as_mut().and_then(|a| a.error.take())) {
        return Err(error);
    }
    result.map_err(Error::from)
}

pub(crate) fn reached(
    roots: &[String],
    corrected: &[String],
    surviving: &[String],
) -> crate::Result<()> {
    let callback = ACTIVE.with_borrow(|active| active.as_ref().map(|a| (a.hook.clone(), a.phase)));
    if let Some((hook, phase)) = callback {
        let event = CorrectionBoundary {
            phase,
            root_fill_ids: roots.to_vec(),
            corrected_fill_ids: corrected.to_vec(),
            surviving_fill_ids: surviving.to_vec(),
        };
        if let Err(error) = hook(&event) {
            ACTIVE.with_borrow_mut(|a| a.as_mut().unwrap().error = Some(storage_error(error)));
            return Err("FAULT_CORRECTION_CLOSURE");
        }
    }
    Ok(())
}

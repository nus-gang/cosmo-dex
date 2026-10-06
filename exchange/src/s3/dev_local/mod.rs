//! Dormant, single-writer development engine. No networking, standard ACK,
//! physical reservation or automatic recovery. See S3-DEV-LOCAL.md.
mod binding;
mod fs;
mod runtime;
mod store;
pub use binding::{Inputs, Validated};
pub use runtime::{
    Command, Engine, RECOVERY_HISTORY_PAGE_MAX, ReadView, RecoveryAttempt, RecoveryFailure,
    RecoveryHistory, RecoveryObservation, View,
};
#[cfg(feature = "fault-injection")]
use std::sync::Arc;
#[derive(Debug)]
pub enum Error {
    Invalid(&'static str),
    Recovery(&'static str),
    Io(std::io::Error),
    WriterAlreadyRunning,
}
pub type Result<T> = std::result::Result<T, Error>;
impl From<&'static str> for Error {
    fn from(s: &'static str) -> Self {
        Self::Invalid(s)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<super::journal::Error> for Error {
    fn from(e: super::journal::Error) -> Self {
        match e {
            super::journal::Error::Io(e) => Self::Io(e),
            _ => Self::Recovery("ENCODING"),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
/// Hooks exist only in explicit fault builds. No environment behavior in normal builds.
#[cfg(feature = "fault-injection")]
type Hook = Arc<dyn Fn(&str) -> Result<()> + Send + Sync>;
#[cfg(not(feature = "fault-injection"))]
type Hook = ();
#[cfg(feature = "fault-injection")]
thread_local! { static IO_HOOK:std::cell::RefCell<Option<Hook>>=const{std::cell::RefCell::new(None)}; }
fn fault(point: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        return IO_HOOK.with_borrow(|h| match h {
            Some(h) => h(point),
            None => Ok(()),
        });
    }
    #[cfg(not(feature = "fault-injection"))]
    {
        let _ = point;
        Ok(())
    }
}
#[cfg(feature = "fault-injection")]
fn with_hook<T>(hook: &Option<Hook>, f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            IO_HOOK.with_borrow_mut(|h| *h = None);
        }
    }
    IO_HOOK.with_borrow_mut(|h| *h = hook.clone());
    let _reset = Reset;
    f()
}
#[cfg(not(feature = "fault-injection"))]
fn with_hook<T>(_: &Option<Hook>, f: impl FnOnce() -> Result<T>) -> Result<T> {
    f()
}

/// Storage-stage failures always require explicit recovery; Invalid is reserved
/// for uncommitted external input rejection. OS error details are retained.
fn storage_error(e: Error) -> Error {
    match e {
        Error::Invalid(code) => Error::Recovery(code),
        other => other,
    }
}

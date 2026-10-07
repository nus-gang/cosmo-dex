//! Process-owned cooperative shutdown; no allocation or I/O in the handler.
//! Install once, before service startup. No forced cleanup or home mutation.
use std::sync::atomic::{AtomicBool, Ordering};
static CLAIMED: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn request_stop(_: libc::c_int) { STOP.store(true, Ordering::Relaxed); }

pub struct Signals { previous: [(libc::c_int, libc::sigaction); 2] }
impl Signals {
    pub fn install() -> Result<Self, &'static str> {
        // A failed install is terminal too: startup never retries partially
        // installed process-global state or clears a pending stop request.
        if CLAIMED.swap(true, Ordering::AcqRel) { return Err("SIGNALS_ALREADY_CLAIMED"); }
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = request_stop as *const () as usize;
            if libc::sigemptyset(&mut action.sa_mask) != 0 { return Err("SIGNAL_MASK"); }
            action.sa_flags = 0;
            let mut old_int: libc::sigaction = std::mem::zeroed();
            let mut old_term: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGINT, std::ptr::null(), &mut old_int) != 0 ||
               libc::sigaction(libc::SIGTERM, std::ptr::null(), &mut old_term) != 0 {
                return Err("SIGNAL_QUERY");
            }
            if old_int.sa_sigaction != libc::SIG_DFL || old_term.sa_sigaction != libc::SIG_DFL {
                return Err("SIGNAL_OWNER_CONFLICT");
            }
            if libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) != 0 {
                return Err("SIGNAL_INSTALL");
            }
            if libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) != 0 {
                // Fail closed even if rollback itself fails. Caller must exit.
                libc::sigaction(libc::SIGINT, &old_int, std::ptr::null_mut());
                return Err("SIGNAL_INSTALL");
            }
            Ok(Self { previous: [(libc::SIGINT,old_int),(libc::SIGTERM,old_term)] })
        }
    }
    pub fn stop(&self) -> &AtomicBool { &STOP }
}
impl Drop for Signals {
    fn drop(&mut self) {
        STOP.store(true,Ordering::Relaxed);
        // The dedicated executable owns these dispositions. It must not
        // install another signal manager while this guard is alive.
        for (signal, previous) in &self.previous {
            unsafe { libc::sigaction(*signal, previous, std::ptr::null_mut()); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signal_child() {
        let Ok(mode)=std::env::var("NUS73_SIGNAL_TEST") else { return; };
        if mode == "conflict" {
            unsafe { libc::signal(libc::SIGINT,libc::SIG_IGN); }
            assert!(matches!(Signals::install(),Err("SIGNAL_OWNER_CONFLICT")));
            let mut current:libc::sigaction=unsafe{std::mem::zeroed()};
            assert_eq!(unsafe{libc::sigaction(libc::SIGINT,std::ptr::null(),&mut current)},0);
            assert_eq!(current.sa_sigaction,libc::SIG_IGN);
            assert!(Signals::install().is_err());
            return;
        }
        let guard=Signals::install().unwrap();
        assert!(!guard.stop().load(Ordering::Relaxed));
        assert!(Signals::install().is_err());
        let signal=if mode=="int" {libc::SIGINT} else {libc::SIGTERM};
        assert_eq!(unsafe{libc::raise(signal)},0);
        assert!(guard.stop().load(Ordering::Relaxed));
        assert_eq!(unsafe{libc::raise(signal)},0);
        assert!(guard.stop().load(Ordering::Relaxed));
        drop(guard);
        for signal in [libc::SIGINT,libc::SIGTERM] {
            let mut current:libc::sigaction=unsafe{std::mem::zeroed()};
            assert_eq!(unsafe{libc::sigaction(signal,std::ptr::null(),&mut current)},0);
            assert_eq!(current.sa_sigaction,libc::SIG_DFL);
        }
        assert!(Signals::install().is_err());
    }
    #[test]
    fn signals_are_latched_restored_and_never_reinstalled() {
        for mode in ["int","term","conflict"] {
            let out=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","tests::signal_child","--test-threads=1"])
                .env("NUS73_SIGNAL_TEST",mode).output().unwrap();
            assert!(out.status.success(),"{mode}: {}",String::from_utf8_lossy(&out.stderr));
        }
    }
}

//! Fault-build-only immediate process exit at an existing C storage hook.
//! Internal API: caller must durably reserve command/selector evidence first.
//! Exit 86 is a process observation, never proof of persistence or DEV success.
#[path = "storage_fault.rs"]
mod storage_fault;
use nus_exchange_contract::s3::dev_local::{Error, Result};
use std::sync::Arc;
pub use storage_fault::Report;
use storage_fault::StorageFault;

pub const CRASH_EXIT: i32 = 86;
pub struct StorageCrash {
    selector: Arc<StorageFault>,
}
impl StorageCrash {
    pub fn new(point: &str, occurrence: u32, enable: bool, allow: bool) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            selector: StorageFault::new(point, occurrence, enable, allow)?,
        }))
    }
    /// `_exit` deliberately skips Rust destructors and stdio flushing. It does
    /// not emulate power loss, kernel failure or disk-cache loss.
    pub fn hook(self: &Arc<Self>) -> Arc<dyn Fn(&str) -> Result<()> + Send + Sync> {
        let hook = self.selector.hook();
        Arc::new(move |point| match hook(point) {
            Err(Error::Io(_)) => unsafe { libc::_exit(CRASH_EXIT) },
            result => result,
        })
    }
    /// A returned report can only mean the selected crash was not reached.
    /// Close retained hooks even if the command returns or unwinds early.
    pub fn finish(&self) -> Result<Report> {
        self.selector.finish()
    }
}

/// Internal command scope. A durable crash-specific reservation is required
/// before calling; the ordinary IO report schema must not be used for crashes.
/// If the selected point is reached, this process does not return or unwind.
/// Otherwise retained hooks are closed and removed on return or panic.
/// Any error requires dropping the exclusively owned Engine; never retry.
pub fn run_command<T>(
    engine: &nus_exchange_contract::s3::dev_local::Engine,
    crash: Arc<StorageCrash>,
    command: impl FnOnce() -> Result<T>,
) -> Result<(Result<T>, Report)> {
    scoped(&crash, |hook| engine.set_fault_hook(hook), command)
}
/// Reserve canonical public command bytes before installing the crash hook.
/// An exit leaves only `reserved`; missing final is UNKNOWN, including exit86.
/// Evidence failures prevent the command; preserve the file and never retry.
pub fn run_recorded_command<T>(
    engine: &nus_exchange_contract::s3::dev_local::Engine,
    crash: Arc<StorageCrash>,
    directory: &std::path::Path,
    command_bytes: &[u8],
    command: impl FnOnce() -> Result<T>,
) -> Result<(Result<T>, Report)> {
    recorded(
        &crash,
        directory,
        command_bytes,
        || run_command(engine, crash.clone(), command),
        |f| f.sync_all(),
    )
}
fn recorded<T>(
    crash: &Arc<StorageCrash>,
    directory: &std::path::Path,
    command_bytes: &[u8],
    action: impl FnOnce() -> Result<T>,
    sync: impl FnMut(&std::fs::File) -> std::io::Result<()>,
) -> Result<T> {
    storage_fault::recorded_mode(
        &crash.selector,
        directory,
        command_bytes,
        action,
        sync,
        true,
    )
}
type Hook = Arc<dyn Fn(&str) -> Result<()> + Send + Sync>;
fn scoped<T>(
    crash: &Arc<StorageCrash>,
    install: impl FnMut(Option<Hook>) -> Result<()>,
    command: impl FnOnce() -> Result<T>,
) -> Result<(Result<T>, Report)> {
    storage_fault::scoped_with_hook(&crash.selector, crash.hook(), install, command)
}

#[cfg(test)]
mod crash_tests {
    use super::*;
    #[allow(dead_code)]
    fn typecheck(
        engine: &nus_exchange_contract::s3::dev_local::Engine,
        f: &Arc<StorageCrash>,
    ) -> Result<()> {
        engine.set_fault_hook(Some(f.hook()))?;
        engine.set_fault_hook(None)
    }
    #[test]
    fn options_and_closed_or_unknown_trace_never_exit() {
        for (p, n, a, b) in [
            ("before_wal", 1, false, true),
            ("before_wal", 1, true, false),
            ("unknown", 1, true, true),
            ("before_wal", 0, true, true),
            ("before_wal", 1025, true, true),
        ] {
            assert!(StorageCrash::new(p, n, a, b).is_err());
        }
        let f = StorageCrash::new("before_wal", 1, true, true).unwrap();
        assert!(f.hook()("unknown").is_err());
        assert!(!f.finish().unwrap().injected);
        assert!(f.hook()("before_wal").is_err());
    }
    #[test]
    fn missing_point_closes_without_claiming_crash() {
        let f = StorageCrash::new("before_wal", 2, true, true).unwrap();
        let h = f.hook();
        h("candidate_verified").unwrap();
        h("before_wal").unwrap();
        assert_eq!(
            f.finish().unwrap(),
            Report {
                visits: 2,
                matching_visits: 1,
                injected: false
            }
        );
        assert!(h("before_wal").is_err());
    }
    #[test]
    fn child_exit_boundary() {
        let Ok(root) = std::env::var("SRE_CRASH_UNIT_ROOT") else {
            return;
        };
        struct DropMarker(std::path::PathBuf);
        impl Drop for DropMarker {
            fn drop(&mut self) {
                std::fs::write(&self.0, b"drop").unwrap();
            }
        }
        let root = std::path::PathBuf::from(root);
        let _marker = DropMarker(root.join("destructor"));
        let f = StorageCrash::new("before_wal", 2, true, true).unwrap();
        let held = std::cell::RefCell::new(None::<Hook>);
        let _: Result<(Result<()>, Report)> = scoped(
            &f,
            |hook| {
                if hook.is_none() {
                    std::fs::write(root.join("removed"), b"removed").unwrap();
                }
                *held.borrow_mut() = hook;
                Ok(())
            },
            || {
                let h = held.borrow();
                let h = h.as_ref().unwrap();
                h("candidate_verified")?;
                h("before_wal")?;
                // Unit-test reachability evidence only; no durability claim.
                std::fs::write(root.join("before"), b"second matching visit next").unwrap();
                h("before_wal")
            },
        );
        std::fs::write(root.join("after"), b"unexpected").unwrap();
        panic!("crash returned");
    }
    #[test]
    fn isolated_child_exits_at_exact_visit_without_unwind() {
        let root = std::env::temp_dir().join(format!("sre-crash-unit-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_tests::child_exit_boundary", "--nocapture"])
            .env_clear()
            .env("SRE_CRASH_UNIT_ROOT", &root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("child timeout");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(CRASH_EXIT));
        assert_eq!(
            std::fs::read(root.join("before")).unwrap(),
            b"second matching visit next"
        );
        assert!(!root.join("removed").exists());
        assert!(!root.join("after").exists());
        assert!(!root.join("destructor").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod crash_scope_tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    #[test]
    fn unreached_success_and_error_remove_and_close_once() {
        for fail in [false, true] {
            let c = StorageCrash::new("before_wal", 2, true, true).unwrap();
            let held = RefCell::new(None::<Hook>);
            let removed = Cell::new(0);
            let (out, report) = scoped(
                &c,
                |h| {
                    if h.is_some() {
                        *held.borrow_mut() = h
                    } else {
                        removed.set(removed.get() + 1)
                    }
                    Ok(())
                },
                || {
                    held.borrow().as_ref().unwrap()("before_wal")?;
                    if fail {
                        Err(Error::Recovery("COMMAND"))
                    } else {
                        Ok(7)
                    }
                },
            )
            .unwrap();
            assert_eq!(out.is_err(), fail);
            assert!(!report.injected);
            assert_eq!(report.matching_visits, 1);
            assert_eq!(removed.get(), 1);
            assert!(held.borrow().as_ref().unwrap()("before_wal").is_err());
            assert!(scoped(&c, |_| panic!("reinstall"), || Ok(())).is_err());
        }
    }
    #[test]
    fn panic_closes_retained_hook_and_removes_before_unwind() {
        let c = StorageCrash::new("before_wal", 1, true, true).unwrap();
        let held = RefCell::new(None::<Hook>);
        let removed = Cell::new(false);
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<(Result<()>, Report)> = scoped(
                &c,
                |h| {
                    if h.is_some() {
                        *held.borrow_mut() = h
                    } else {
                        removed.set(true)
                    }
                    Ok(())
                },
                || panic!("command"),
            );
        }));
        assert!(out.is_err());
        assert!(removed.get());
        assert!(held.borrow().as_ref().unwrap()("before_wal").is_err());
        assert!(scoped(&c, |_| panic!("reinstall"), || Ok(())).is_err());
    }
    #[test]
    fn install_and_remove_failure_are_closed() {
        for install_fail in [true, false] {
            let c = StorageCrash::new("before_wal", 1, true, true).unwrap();
            let calls = Cell::new(0);
            let result = scoped(
                &c,
                |h| {
                    if h.is_some() == install_fail {
                        Err(Error::Recovery("SETTER"))
                    } else {
                        Ok(())
                    }
                },
                || {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            );
            assert!(matches!(result, Err(Error::Recovery("SETTER"))));
            assert_eq!(calls.get(), if install_fail { 0 } else { 1 });
            assert!(c.hook()("before_wal").is_err());
            assert!(scoped(&c, |_| panic!("reinstall"), || Ok(())).is_err());
        }
    }
}

#[cfg(test)]
mod crash_record_tests {
    use super::*;
    use std::{cell::Cell, os::unix::fs::PermissionsExt};
    fn command() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"effect":"IMMEDIATE_EXIT","exit_code":86,"point":"before_wal","occurrence":"1","command":"synthetic"})).unwrap()
    }
    fn root(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("crash-record-{}-{tag}", std::process::id()));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p.canonicalize().unwrap()
    }
    fn inspect_real(root: &std::path::Path, phase: &str) {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ops/s3-local/check_real_crash_report.py");
        let result = std::process::Command::new("python3")
            .arg(script)
            .arg(root)
            .arg(nus_exchange_contract::s3::journal::sha256(&command()))
            .arg(phase)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, b"REAL_CRASH_WRITER_REPORT_PASS\n");
    }
    #[test]
    fn reservation_sync_precedes_action_and_unreached_final_is_distinct() {
        let p = root("returned");
        let c = StorageCrash::new("before_wal", 1, true, true).unwrap();
        let syncs = Cell::new(0);
        recorded(
            &c,
            &p,
            &command(),
            || {
                assert_eq!(syncs.get(), 2);
                Ok(())
            },
            |f| {
                syncs.set(syncs.get() + 1);
                f.sync_all()
            },
        )
        .unwrap();
        assert_eq!(syncs.get(), 4);
        let raw = std::fs::read_to_string(p.join("storage-crash.jsonl")).unwrap();
        let rows: Vec<serde_json::Value> = raw
            .lines()
            .map(|x| serde_json::from_str(x).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["phase"], "reserved");
        assert_eq!(rows[1]["phase"], "scope_returned");
        for r in rows {
            assert_eq!(r["schema"], "s3-local-storage-crash/1");
            assert_eq!(r["crash_verified"], false);
            assert_eq!(
                r["command_sha256"],
                nus_exchange_contract::s3::journal::sha256(&command())
            );
        }
        inspect_real(&p, "scope_returned");
        assert!(!p.join("storage-fault.jsonl").exists());
        assert!(
            recorded::<()>(
                &StorageCrash::new("before_wal", 1, true, true).unwrap(),
                &p,
                &command(),
                || panic!("retry"),
                |f| f.sync_all()
            )
            .is_err()
        );
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn mismatched_selector_and_sync_failure_prevent_action() {
        for fail_at in [1, 2] {
            let p = root(&format!("sync-{fail_at}"));
            let c = StorageCrash::new("before_wal", 1, true, true).unwrap();
            assert!(recorded::<()>(&c, &p, b"{}", || panic!("unbound"), |f| f.sync_all()).is_err());
            assert_eq!(std::fs::read_dir(&p).unwrap().count(), 0);
            let calls = Cell::new(0);
            assert!(
                recorded::<()>(
                    &c,
                    &p,
                    &command(),
                    || panic!("unsynced"),
                    |f| {
                        calls.set(calls.get() + 1);
                        if calls.get() == fail_at {
                            Err(std::io::Error::other("sync"))
                        } else {
                            f.sync_all()
                        }
                    }
                )
                .is_err()
            );
            assert!(p.join("storage-crash.jsonl").exists());
            assert!(
                recorded::<()>(&c, &p, &command(), || panic!("retry"), |f| f.sync_all()).is_err()
            );
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn crash_reservation_child() {
        let Ok(p) = std::env::var("SRE_CRASH_RECORD_ROOT") else {
            return;
        };
        let p = std::path::PathBuf::from(p);
        let c = StorageCrash::new("before_wal", 1, true, true).unwrap();
        let _: Result<()> = recorded(
            &c,
            &p,
            &command(),
            || c.hook()("before_wal"),
            |f| f.sync_all(),
        );
        panic!("exit returned");
    }
    #[test]
    fn real_exit_leaves_only_reservation_and_never_claims_success() {
        let p = root("child");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_record_tests::crash_reservation_child"])
            .env_clear()
            .env("SRE_CRASH_RECORD_ROOT", &p)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break s;
            }
            if std::time::Instant::now() > until {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("timeout")
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(86));
        let raw = std::fs::read_to_string(p.join("storage-crash.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 1);
        let v: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(v["phase"], "reserved");
        assert_eq!(v["crash_verified"], false);
        assert_eq!(v["injected"], false);
        inspect_real(&p, "UNKNOWN");
        std::fs::remove_dir_all(p).unwrap();
    }
}

//! SRE F14 adapter: only the approved Prepare closure boundary is injectable.
//! No closure calculation, storage implementation, environment switch or CLI.
use nus_exchange_contract::s3::dev_local::{
    CorrectionHook, CorrectionPhase, Engine, Error, Result,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub prepare_visits: u32,
    pub replay_visits: u32,
    pub injected: bool,
}
struct State {
    report: Report,
    claimed: bool,
    closed: bool,
}
pub struct CorrectionFault {
    occurrence: u32,
    state: Mutex<State>,
}
impl CorrectionFault {
    pub fn new(occurrence: u32, enable: bool, allow: bool) -> Result<Arc<Self>> {
        if !enable || !allow || !(1..=1024).contains(&occurrence) {
            return Err(Error::Invalid("F14_OPTIONS"));
        }
        Ok(Arc::new(Self {
            occurrence,
            state: Mutex::new(State {
                report: Report {
                    prepare_visits: 0,
                    replay_visits: 0,
                    injected: false,
                },
                claimed: false,
                closed: false,
            }),
        }))
    }
    pub fn hook(self: &Arc<Self>) -> CorrectionHook {
        let selected = self.clone();
        Arc::new(move |boundary| {
            let mut s = selected
                .state
                .lock()
                .map_err(|_| Error::Recovery("F14_POISONED"))?;
            if s.closed {
                return Err(Error::Recovery("F14_CLOSED"));
            }
            if s.report.prepare_visits + s.report.replay_visits == 65536 {
                s.closed = true;
                return Err(Error::Recovery("F14_TRACE_LIMIT"));
            }
            match boundary.phase {
                CorrectionPhase::SemanticReplay => {
                    s.report.replay_visits += 1;
                    Ok(())
                }
                CorrectionPhase::Prepare => {
                    s.report.prepare_visits += 1;
                    if s.report.prepare_visits == selected.occurrence {
                        s.report.injected = true;
                        s.closed = true;
                        Err(Error::Recovery("SRE_F14_PREPARE_INJECTED"))
                    } else {
                        Ok(())
                    }
                }
            }
        })
    }
    pub fn finish(&self) -> Result<Report> {
        let mut s = self
            .state
            .lock()
            .map_err(|_| Error::Recovery("F14_POISONED"))?;
        s.closed = true;
        Ok(s.report)
    }
}
/// Exclusively owned fault-build Engine only. Never invoke Engine APIs in hook.
/// Caller must drop the Engine after this diagnostic command, even if missed.
pub fn run_command<T>(
    engine: &Engine,
    fault: &Arc<CorrectionFault>,
    command: impl FnOnce() -> Result<T>,
) -> Result<(Result<T>, Report)> {
    scoped(fault, |hook| engine.set_correction_hook(hook), command)
}
fn scoped<T>(
    fault: &Arc<CorrectionFault>,
    mut install: impl FnMut(Option<CorrectionHook>) -> Result<()>,
    command: impl FnOnce() -> Result<T>,
) -> Result<(Result<T>, Report)> {
    {
        let mut s = fault
            .state
            .lock()
            .map_err(|_| Error::Recovery("F14_POISONED"))?;
        if s.claimed || s.closed || s.report.prepare_visits + s.report.replay_visits != 0 {
            return Err(Error::Recovery("F14_REUSED"));
        }
        s.claimed = true;
    }
    if let Err(e) = install(Some(fault.hook())) {
        fault.finish()?;
        return Err(e);
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(command));
    let report = fault.finish();
    let removed = install(None);
    match result {
        Err(p) => std::panic::resume_unwind(p),
        Ok(r) => {
            removed?;
            Ok((r, report?))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use nus_exchange_contract::s3::dev_local::CorrectionBoundary;
    fn boundary(phase: CorrectionPhase) -> CorrectionBoundary {
        CorrectionBoundary {
            phase,
            root_fill_ids: vec!["root".into()],
            corrected_fill_ids: vec!["dependent".into()],
            surviving_fill_ids: vec!["independent".into()],
        }
    }
    #[test]
    fn exact_prepare_only_and_no_boundary_mutation() {
        let f = CorrectionFault::new(2, true, true).unwrap();
        let h = f.hook();
        let b = boundary(CorrectionPhase::Prepare);
        let original = b.clone();
        h(&boundary(CorrectionPhase::SemanticReplay)).unwrap();
        h(&b).unwrap();
        h(&boundary(CorrectionPhase::SemanticReplay)).unwrap();
        assert!(matches!(
            h(&b),
            Err(Error::Recovery("SRE_F14_PREPARE_INJECTED"))
        ));
        assert_eq!(b, original);
        assert!(h(&b).is_err());
        assert_eq!(
            f.finish().unwrap(),
            Report {
                prepare_visits: 2,
                replay_visits: 2,
                injected: true
            }
        );
    }
    #[test]
    fn explicit_options_missed_and_trace_bound() {
        for (n, a, b) in [
            (0, true, true),
            (1025, true, true),
            (1, false, true),
            (1, true, false),
        ] {
            assert!(CorrectionFault::new(n, a, b).is_err());
        }
        let f = CorrectionFault::new(1, true, true).unwrap();
        let h = f.hook();
        for _ in 0..65536 {
            h(&boundary(CorrectionPhase::SemanticReplay)).unwrap();
        }
        assert!(matches!(
            h(&boundary(CorrectionPhase::Prepare)),
            Err(Error::Recovery("F14_TRACE_LIMIT"))
        ));
        assert!(!f.finish().unwrap().injected);
    }
    #[test]
    fn scope_closes_retained_hook_on_return_error_and_panic() {
        for mode in 0..3 {
            let f = CorrectionFault::new(1, true, true).unwrap();
            let retained = f.hook();
            let calls = std::cell::RefCell::new(Vec::new());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scoped(
                    &f,
                    |h| {
                        calls.borrow_mut().push(h.is_some());
                        Ok(())
                    },
                    || -> Result<()> {
                        match mode {
                            0 => Ok(()),
                            1 => Err(Error::Invalid("COMMAND")),
                            _ => panic!("expected"),
                        }
                    },
                )
            }));
            assert_eq!(*calls.borrow(), vec![true, false]);
            assert_eq!(result.is_err(), mode == 2);
            assert!(retained(&boundary(CorrectionPhase::Prepare)).is_err());
            assert!(scoped(&f, |_| panic!("reinstall"), || Ok(())).is_err());
        }
    }
    #[test]
    fn install_and_remove_failure_never_reuse() {
        for fail_install in [true, false] {
            let f = CorrectionFault::new(1, true, true).unwrap();
            let ran = std::cell::Cell::new(false);
            assert!(
                scoped(
                    &f,
                    |h| if h.is_some() == fail_install {
                        Err(Error::Recovery("SETTER"))
                    } else {
                        Ok(())
                    },
                    || {
                        ran.set(true);
                        Ok(())
                    }
                )
                .is_err()
            );
            assert_eq!(ran.get(), !fail_install);
            assert!(f.hook()(&boundary(CorrectionPhase::Prepare)).is_err());
        }
    }
}

/// Persist bounded, canonical, non-secret command provenance before installing
/// the F14 hook. Missing final is UNKNOWN; scope_returned is not Apply success.
pub fn run_recorded_command<T>(
    engine: &Engine,
    fault: &Arc<CorrectionFault>,
    directory: &std::path::Path,
    command_bytes: &[u8],
    command: impl FnOnce() -> Result<T>,
) -> Result<(Result<T>, Report)> {
    recorded(
        fault,
        directory,
        command_bytes,
        || run_command(engine, fault, command),
        |f| f.sync_all(),
    )
}
fn recorded<T>(
    fault: &Arc<CorrectionFault>,
    directory: &std::path::Path,
    command_bytes: &[u8],
    action: impl FnOnce() -> Result<T>,
    mut sync: impl FnMut(&std::fs::File) -> std::io::Result<()>,
) -> Result<T> {
    use std::{
        fs::{File, OpenOptions},
        io::Write,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };
    if command_bytes.is_empty() || command_bytes.len() > 16384 {
        return Err(Error::Invalid("FAULT_COMMAND_BYTES"));
    }
    let value: serde_json::Value =
        serde_json::from_slice(command_bytes).map_err(|_| Error::Invalid("F14_COMMAND_UNBOUND"))?;
    if !value.is_object()
        || value["effect"] != "F14_PREPARE_ERROR"
        || value["phase"] != "Prepare"
        || value["occurrence"] != fault.occurrence.to_string()
        || serde_json::to_vec(&value).map_err(|_| Error::Invalid("F14_COMMAND_UNBOUND"))?
            != command_bytes
    {
        return Err(Error::Invalid("F14_COMMAND_UNBOUND"));
    }
    use base64::Engine as _;
    let digest = nus_exchange_contract::s3::journal::sha256(command_bytes);
    let encoded = base64::engine::general_purpose::STANDARD.encode(command_bytes);
    {
        let s = fault
            .state
            .lock()
            .map_err(|_| Error::Recovery("STORAGE_FAULT_POISONED"))?;
        if s.claimed || s.closed || s.report.prepare_visits + s.report.replay_visits != 0 {
            return Err(Error::Invalid("FAULT_REPORT_USED"));
        }
    }
    if !directory.is_absolute() || directory.canonicalize()? != directory {
        return Err(Error::Invalid("FAULT_REPORT_ROOT"));
    }
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(directory)?;
    let rm = root.metadata()?;
    if rm.uid() != unsafe { libc::geteuid() } || rm.mode() & 0o7777 != 0o700 {
        return Err(Error::Invalid("FAULT_REPORT_ROOT"));
    }
    let name = c"correction-fault.jsonl";
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let fm = file.metadata()?;
    let mut append = |phase: &str, report: Report| -> Result<()> {
        let check = || -> Result<()> {
            let r = std::fs::symlink_metadata(directory)?;
            let f = std::fs::symlink_metadata(directory.join("correction-fault.jsonl"))?;
            if (r.dev(), r.ino(), r.mode(), r.uid()) != (rm.dev(), rm.ino(), rm.mode(), rm.uid())
                || (f.dev(), f.ino(), f.mode(), f.uid(), f.nlink())
                    != (fm.dev(), fm.ino(), fm.mode(), fm.uid(), 1)
                || fm.mode() & 0o7777 != 0o600
            {
                return Err(Error::Invalid("FAULT_REPORT_CHANGED"));
            }
            Ok(())
        };
        check()?;
        let value = serde_json::json!({"schema":"s3-local-correction-fault/1",
            "phase":phase,"command_sha256":digest,"command_base64":encoded,
            "effect":"F14_PREPARE_ERROR","selected_phase":"Prepare","occurrence":fault.occurrence,
            "prepare_visits":report.prepare_visits,"replay_visits":report.replay_visits,
            "injected":report.injected,"command_success_verified":false,
            "durable_ack":false,"DEV":"NOT_RUN"});
        let raw = serde_json::to_vec(&value).map_err(|_| Error::Invalid("FAULT_REPORT_JSON"))?;
        file.write_all(&raw)?;
        file.write_all(b"\n")?;
        sync(&file)?;
        sync(&root)?;
        check()
    };
    append(
        "reserved",
        Report {
            prepare_visits: 0,
            replay_visits: 0,
            injected: false,
        },
    )?;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
    let report = fault.finish()?;
    let phase = match &outcome {
        Ok(Ok(_)) => "scope_returned",
        Ok(Err(_)) => "scope_error",
        Err(_) => "panic",
    };
    let persisted = append(phase, report);
    match outcome {
        Err(p) => std::panic::resume_unwind(p),
        Ok(result) => {
            persisted?;
            result
        }
    }
}
#[cfg(test)]
mod record_tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    fn root() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "f14-record-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p.canonicalize().unwrap()
    }
    fn bytes() -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"effect":"F14_PREPARE_ERROR","phase":"Prepare","occurrence":"1"}),
        )
        .unwrap()
    }
    #[test]
    fn report_binds_phase_and_injection_and_retains_panic() {
        for mode in 0..3 {
            let p = root();
            let f = CorrectionFault::new(1, true, true).unwrap();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                recorded(
                    &f,
                    &p,
                    &bytes(),
                    || -> Result<()> {
                        if mode == 2 {
                            panic!("expected")
                        }
                        if mode == 1 {
                            f.hook()(&nus_exchange_contract::s3::dev_local::CorrectionBoundary {
                                phase: CorrectionPhase::Prepare,
                                root_fill_ids: vec![],
                                corrected_fill_ids: vec![],
                                surviving_fill_ids: vec![],
                            })?;
                        }
                        Ok(())
                    },
                    |file| file.sync_all(),
                )
            }));
            assert_eq!(r.is_err(), mode == 2);
            let raw = fs::read_to_string(p.join("correction-fault.jsonl")).unwrap();
            let v: Vec<serde_json::Value> = raw
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            assert_eq!(v.len(), 2);
            assert_eq!(v[0]["phase"], "reserved");
            assert_eq!(
                v[1]["phase"],
                ["scope_returned", "scope_error", "panic"][mode]
            );
            assert_eq!(v[1]["injected"], mode == 1);
            assert_eq!(v[1]["replay_visits"], 0);
            assert_eq!(
                v[0]["command_sha256"],
                nus_exchange_contract::s3::journal::sha256(&bytes())
            );
            let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ops/s3-local/check_real_correction_report.py");
            let checked = std::process::Command::new("python3")
                .arg(script)
                .arg(&p)
                .arg(nus_exchange_contract::s3::journal::sha256(&bytes()))
                .arg(["scope_returned", "scope_error", "panic"][mode])
                .arg(if mode == 1 { "true" } else { "false" })
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .output()
                .unwrap();
            assert!(
                checked.status.success(),
                "{}",
                String::from_utf8_lossy(&checked.stderr)
            );
            assert_eq!(checked.stdout, b"REAL_F14_WRITER_REPORT_PASS\n");
            assert!(
                recorded(
                    &f,
                    &p,
                    &bytes(),
                    || -> Result<()> { panic!("retry") },
                    |f| f.sync_all()
                )
                .is_err()
            );
            fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn unbound_and_sync_failure_prevent_action() {
        let p = root();
        let f = CorrectionFault::new(1, true, true).unwrap();
        assert!(
            recorded(
                &f,
                &p,
                b"{}",
                || -> Result<()> { panic!("action") },
                |f| f.sync_all()
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&p).unwrap().count(), 0);
        assert!(
            recorded(
                &f,
                &p,
                &bytes(),
                || -> Result<()> { panic!("action") },
                |_| Err(std::io::Error::other("sync"))
            )
            .is_err()
        );
        assert!(p.join("correction-fault.jsonl").exists());
        assert!(
            recorded(
                &f,
                &p,
                &bytes(),
                || -> Result<()> { panic!("retry") },
                |f| f.sync_all()
            )
            .is_err()
        );
        fs::remove_dir_all(p).unwrap();
    }
}

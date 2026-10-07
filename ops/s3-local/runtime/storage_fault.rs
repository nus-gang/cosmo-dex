//! SRE fault-build adapter for C's existing set_fault_hook API.
//! No environment trigger, process termination, sleep or storage implementation.
use nus_exchange_contract::s3::dev_local::{Error, Result};
use std::sync::{Arc, Mutex};

const POINTS: &[&str] = &[
    "candidate_verified", "evidence_write", "evidence_complete", "before_wal",
    "partial_wal", "wal_sync", "after_wal_sync", "marker_sync", "after_marker_sync",
    "after_marker_rename", "marker_dir_sync", "after_marker_dir_sync", "after_commit",
    "before_publish", "before_response", "file_sync", "publish_dir_sync",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report { pub visits: u32, pub matching_visits: u32, pub injected: bool }
struct State { report: Report, closed: bool, claimed: bool }
/// Host errno injection only; no disk filling, quotas or OS configuration changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoFault { Generic, Enospc, Edquot, Eio }
impl IoFault {
    pub fn label(self) -> &'static str {
        match self { Self::Generic=>"GENERIC", Self::Enospc=>"ENOSPC",
            Self::Edquot=>"EDQUOT", Self::Eio=>"EIO" }
    }
    fn error(self) -> std::io::Error {
        match self {
            Self::Generic => std::io::Error::other("SRE_INJECTED_IO"),
            Self::Enospc => std::io::Error::from_raw_os_error(libc::ENOSPC),
            Self::Edquot => std::io::Error::from_raw_os_error(libc::EDQUOT),
            Self::Eio => std::io::Error::from_raw_os_error(libc::EIO),
        }
    }
}
pub struct StorageFault { point: String, occurrence: u32, io_fault: IoFault, state: Mutex<State> }
impl StorageFault {
    pub fn new(point: &str, occurrence: u32, enable: bool, allow: bool) -> Result<Arc<Self>> {
        Self::with_io_fault(point, occurrence, IoFault::Generic, enable, allow)
    }
    /// Internal fault-build API. Errno reports require a bound canonical command.
    pub fn with_io_fault(point: &str, occurrence: u32, io_fault: IoFault,
        enable: bool, allow: bool) -> Result<Arc<Self>> {
        if !enable || !allow || !POINTS.contains(&point) || !(1..=1024).contains(&occurrence) {
            return Err(Error::Invalid("STORAGE_FAULT_OPTIONS"));
        }
        Ok(Arc::new(Self { point: point.into(), occurrence, io_fault,
            state: Mutex::new(State { report: Report { visits: 0, matching_visits: 0,
                injected: false }, closed: false, claimed: false }) }))
    }
    /// Install only around one explicit command in an isolated fault-build Engine.
    /// The caller owns command identity, before/after evidence and hook removal.
    pub fn hook(self: &Arc<Self>) -> Arc<dyn Fn(&str) -> Result<()> + Send + Sync> {
        let selected = self.clone();
        Arc::new(move |point| selected.visit(point))
    }
    fn visit(&self, point: &str) -> Result<()> {
        let mut s = self.state.lock().map_err(|_| Error::Recovery("STORAGE_FAULT_POISONED"))?;
        if s.closed { return Err(Error::Recovery("STORAGE_FAULT_CLOSED")); }
        if !POINTS.contains(&point) || s.report.visits == 65536 {
            s.closed = true;
            return Err(Error::Recovery("STORAGE_FAULT_TRACE"));
        }
        s.report.visits += 1;
        if point == self.point {
            s.report.matching_visits += 1;
            if s.report.matching_visits == self.occurrence {
                s.report.injected = true;
                s.closed = true;
                return Err(Error::Io(self.io_fault.error()));
            }
        }
        Ok(())
    }
    /// Close further visits and read the observation, including a missing point.
    /// `injected` reports a hook error only; it is not a crash/DEV/Fxx verdict.
    pub fn finish(&self) -> Result<Report> {
        let mut s = self.state.lock().map_err(|_| Error::Recovery("STORAGE_FAULT_POISONED"))?;
        s.closed = true;
        Ok(s.report)
    }
}
/// A single command scope for an isolated, exclusively owned fault-build Engine.
/// No other caller may install hooks or execute commands concurrently. A failed
/// command or removal requires the caller to drop the Engine, never resume it.
/// This does not install a hook in the ordinary worker binary.
pub fn run_command<T>(engine: &nus_exchange_contract::s3::dev_local::Engine,
    fault: Arc<StorageFault>, command: impl FnOnce() -> Result<T>) -> Result<(Result<T>, Report)> {
    scoped(&fault, |hook| engine.set_fault_hook(hook), command)
}
/// Reserve the exact bounded command envelope before hook installation.
/// Compute its SHA256 here; callers cannot supply an unrelated digest.
/// The envelope is provenance, not an authorization check; it must contain no secrets.
/// Missing/torn final records (including SIGKILL) mean UNKNOWN; never auto-retry.
pub fn run_recorded_command<T>(engine: &nus_exchange_contract::s3::dev_local::Engine,
    fault: Arc<StorageFault>, directory: &std::path::Path, command_bytes: &[u8],
    command: impl FnOnce() -> Result<T>) -> Result<(Result<T>, Report)> {
    recorded(&fault, directory, command_bytes, || run_command(engine, fault.clone(), command),
        |file| file.sync_all())
}
fn recorded<T>(fault: &Arc<StorageFault>, directory: &std::path::Path, command_bytes: &[u8],
    action: impl FnOnce() -> Result<T>, mut sync: impl FnMut(&std::fs::File) -> std::io::Result<()>) -> Result<T> {
    use std::{fs::{File,OpenOptions}, io::Write,
        os::{fd::{AsRawFd,FromRawFd}, unix::fs::{MetadataExt,OpenOptionsExt}}};
    if command_bytes.is_empty() || command_bytes.len() > 16384 {
        return Err(Error::Invalid("FAULT_COMMAND_BYTES"));
    }
    // Preserve legacy generic v2. Errno v3 must bind the selector in the
    // canonical command hashed below, before any filesystem access or effect.
    if fault.io_fault != IoFault::Generic {
        let value:serde_json::Value=serde_json::from_slice(command_bytes)
            .map_err(|_| Error::Invalid("FAULT_REPORT_ERRNO_UNBOUND"))?;
        if !value.is_object() || value["io_fault"] != fault.io_fault.label() ||
            value["point"] != fault.point || value["occurrence"] != fault.occurrence.to_string() ||
            serde_json::to_vec(&value).map_err(|_| Error::Invalid("FAULT_REPORT_ERRNO_UNBOUND"))? != command_bytes {
            return Err(Error::Invalid("FAULT_REPORT_ERRNO_UNBOUND"));
        }
    }
    use base64::Engine as _;
    let digest = nus_exchange_contract::s3::journal::sha256(command_bytes);
    let encoded = base64::engine::general_purpose::STANDARD.encode(command_bytes);
    {
        let s=fault.state.lock().map_err(|_| Error::Recovery("STORAGE_FAULT_POISONED"))?;
        if s.claimed || s.closed || s.report.visits!=0 { return Err(Error::Invalid("FAULT_REPORT_USED")); }
    }
    if !directory.is_absolute() || directory.canonicalize()? != directory {
        return Err(Error::Invalid("FAULT_REPORT_ROOT"));
    }
    let root=OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY|libc::O_NOFOLLOW).open(directory)?;
    let rm=root.metadata()?;
    if rm.uid()!=unsafe {libc::geteuid()} || rm.mode() & 0o7777 != 0o700 {
        return Err(Error::Invalid("FAULT_REPORT_ROOT"));
    }
    let fd=unsafe {libc::openat(root.as_raw_fd(),c"storage-fault.jsonl".as_ptr(),
        libc::O_WRONLY|libc::O_CREAT|libc::O_EXCL|libc::O_NOFOLLOW|libc::O_CLOEXEC,0o600)};
    if fd<0 { return Err(std::io::Error::last_os_error().into()); }
    let mut file=unsafe {File::from_raw_fd(fd)};
    let fm=file.metadata()?;
    let mut append=|phase:&str, report:Report| -> Result<()> {
        let check=|| -> Result<()> {
            let r=std::fs::symlink_metadata(directory)?;
            let f=std::fs::symlink_metadata(directory.join("storage-fault.jsonl"))?;
            if (r.dev(),r.ino(),r.mode(),r.uid())!=(rm.dev(),rm.ino(),rm.mode(),rm.uid()) ||
                (f.dev(),f.ino(),f.mode(),f.uid(),f.nlink())!=(fm.dev(),fm.ino(),fm.mode(),fm.uid(),1) ||
                fm.mode() & 0o7777 != 0o600 {return Err(Error::Invalid("FAULT_REPORT_CHANGED"));}
            Ok(())
        };
        check()?;
        let mut value=serde_json::json!({"schema":"s3-local-storage-fault/2",
            "phase":phase,"command_sha256":digest,"command_base64":encoded,"point":fault.point,"occurrence":fault.occurrence,
            "visits":report.visits,"matching_visits":report.matching_visits,"injected":report.injected,
            "durable_ack":false,"DEV":"NOT_RUN"});
        if fault.io_fault != IoFault::Generic {
            value["schema"]=serde_json::json!("s3-local-storage-fault/3");
            value["io_fault"]=serde_json::json!(fault.io_fault.label());
        }
        let raw=serde_json::to_vec(&value).map_err(|_| Error::Invalid("FAULT_REPORT_JSON"))?;
        file.write_all(&raw)?; file.write_all(b"\n")?; sync(&file)?; sync(&root)?; check()
    };
    append("reserved",Report{visits:0,matching_visits:0,injected:false})?;
    let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
    let report=fault.finish()?;
    let phase=match &outcome {Ok(Ok(_))=>"scope_returned",Ok(Err(_))=>"scope_error",Err(_)=>"panic"};
    let persisted=append(phase,report);
    match outcome { Err(p)=>std::panic::resume_unwind(p), Ok(result)=>{persisted?; result} }
}
type Hook = Arc<dyn Fn(&str) -> Result<()> + Send + Sync>;
fn scoped<T>(fault: &Arc<StorageFault>, mut install: impl FnMut(Option<Hook>) -> Result<()>,
    command: impl FnOnce() -> Result<T>) -> Result<(Result<T>, Report)> {
    // Claim once before touching the Engine, even when installation fails.
    {
        let mut s = fault.state.lock().map_err(|_| Error::Recovery("STORAGE_FAULT_POISONED"))?;
        if s.closed || s.claimed || s.report.visits != 0 {
            return Err(Error::Recovery("STORAGE_FAULT_REUSED"));
        }
        s.claimed = true;
    }
    if let Err(e) = install(Some(fault.hook())) { fault.finish()?; return Err(e); }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(command));
    // Close retained callback clones before attempting removal.
    let report = fault.finish();
    let removed = install(None);
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
    removed?;
    Ok((outcome.unwrap(), report?))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[allow(dead_code)]
    fn approved_api_typecheck(engine: &nus_exchange_contract::s3::dev_local::Engine, f: &Arc<StorageFault>) -> Result<()> {
        engine.set_fault_hook(Some(f.hook()))?;
        engine.set_fault_hook(None)
    }
    #[test] fn options_are_explicit_and_bounded() {
        for (p,n,a,b) in [("wal_sync",1,false,true),("wal_sync",1,true,false),
            ("F11",1,true,true),("guard_complete",1,true,true),
            ("wal_sync",0,true,true),("wal_sync",1025,true,true)] {
            assert!(StorageFault::new(p,n,a,b).is_err());
        }
    }
    #[test] fn exact_occurrence_errors_once_then_closes() {
        let f=StorageFault::new("file_sync",2,true,true).unwrap(); let h=f.hook();
        h("candidate_verified").unwrap(); h("file_sync").unwrap();
        assert!(matches!(h("file_sync"),Err(Error::Io(_))));
        assert!(matches!(h("before_wal"),Err(Error::Recovery("STORAGE_FAULT_CLOSED"))));
        assert_eq!(f.finish().unwrap(),Report{visits:3,matching_visits:2,injected:true});
    }
    #[test] fn missing_point_and_unknown_trace_do_not_claim_injection() {
        let f=StorageFault::new("wal_sync",1,true,true).unwrap(); let h=f.hook();
        h("candidate_verified").unwrap(); assert!(!f.finish().unwrap().injected);
        assert!(h("wal_sync").is_err());
        let f=StorageFault::new("wal_sync",1,true,true).unwrap();
        assert!(f.hook()("unknown").is_err()); assert!(!f.finish().unwrap().injected);
    }
    #[test] fn concurrent_visits_have_one_injection_and_a_finite_trace() {
        let f=StorageFault::new("wal_sync",1,true,true).unwrap();
        let threads:Vec<_>=(0..8).map(|_| {let h=f.hook();std::thread::spawn(move ||
            matches!(h("wal_sync"),Err(Error::Io(_))))}).collect();
        assert_eq!(threads.into_iter().filter_map(|t|t.join().ok()).filter(|x|*x).count(),1);
        assert_eq!(f.finish().unwrap().visits,1);
        let f=StorageFault::new("wal_sync",1,true,true).unwrap();let h=f.hook();
        for _ in 0..65536 {h("before_wal").unwrap();}
        assert!(h("before_wal").is_err());assert!(!f.finish().unwrap().injected);
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    #[test] fn one_command_and_removal_on_success_or_error() {
        for fail in [false,true] {
            let f=StorageFault::new("wal_sync",1,true,true).unwrap();
            let slot=RefCell::new(None::<Hook>); let calls=Cell::new(0);
            let (out,report)=scoped(&f, |h| { *slot.borrow_mut()=h; Ok(()) }, || {
                calls.set(calls.get()+1);
                if fail { slot.borrow().as_ref().unwrap()("wal_sync")?; }
                Ok(7)
            }).unwrap();
            assert_eq!(out.is_err(),fail); assert_eq!(report.injected,fail);
            assert!(slot.borrow().is_none()); assert_eq!(calls.get(),1);
            assert!(scoped(&f, |_| panic!("reinstalled"), || Ok(())).is_err());
        }
    }
    #[test] fn panic_removes_hook_and_closes_retained_clone() {
        let f=StorageFault::new("wal_sync",1,true,true).unwrap();
        let removed=Cell::new(false); let retained=RefCell::new(None::<Hook>);
        let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _:Result<(Result<()>,Report)>=scoped(&f, |h| {
                if h.is_some() { *retained.borrow_mut()=h; } else { removed.set(true); } Ok(())
            }, || panic!("synthetic command panic"));
        }));
        assert!(outcome.is_err()); assert!(removed.get());
        assert!(retained.borrow().as_ref().unwrap()("wal_sync").is_err());
        assert!(!f.finish().unwrap().injected);
    }
    #[test] fn install_failure_has_no_command_and_is_not_reusable() {
        let f=StorageFault::new("wal_sync",1,true,true).unwrap();
        assert!(scoped(&f, |_| Err(Error::Recovery("INSTALL")), || -> Result<()> {panic!("called")}).is_err());
        assert!(scoped(&f, |_| panic!("retry"), || Ok(())).is_err());
    }
    #[test] fn removal_failure_cannot_be_reported_as_success() {
        let f=StorageFault::new("wal_sync",1,true,true).unwrap();
        assert!(matches!(scoped(&f, |h| if h.is_none() {Err(Error::Recovery("REMOVE"))} else {Ok(())}, || Ok(1)),
            Err(Error::Recovery("REMOVE"))));
        assert!(f.hook()("wal_sync").is_err());
    }
}

#[cfg(test)]
mod record_tests {
    use super::*;
    use std::{cell::Cell, os::unix::fs::PermissionsExt};
    fn root() -> std::path::PathBuf {
        static N:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
        let p=std::env::temp_dir().canonicalize().unwrap().join(format!("fault-record-{}-{}",
            std::process::id(),N.fetch_add(1,std::sync::atomic::Ordering::SeqCst)));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p,std::fs::Permissions::from_mode(0o700)).unwrap(); p
    }
    #[test] fn reserved_before_action_and_panic_or_error_is_preserved() {
        for panic in [false,true] {
            let p=root();let f=StorageFault::new("before_wal",1,true,true).unwrap();
            let out=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                recorded::<()>(&f,&p,b"exact synthetic command",|| {
                    let raw=std::fs::read_to_string(p.join("storage-fault.jsonl")).unwrap();
                    assert_eq!(raw.lines().count(),1); assert!(raw.contains("reserved"));
                    assert!(f.hook()("before_wal").is_err());
                    if panic {panic!("private error must not be serialized")}
                    Err(Error::Recovery("private failure"))
                },|file|file.sync_all())
            }));
            assert!(out.is_err() || out.unwrap().is_err());
            let raw=std::fs::read_to_string(p.join("storage-fault.jsonl")).unwrap();
            let values:Vec<serde_json::Value>=raw.lines().map(|l|serde_json::from_str(l).unwrap()).collect();
            assert_eq!(values.len(),2);
            use base64::Engine as _;
            for v in &values {
                let bytes=base64::engine::general_purpose::STANDARD.decode(v["command_base64"].as_str().unwrap()).unwrap();
                assert_eq!(bytes,b"exact synthetic command");
                assert_eq!(v["command_sha256"],nus_exchange_contract::s3::journal::sha256(&bytes));
                assert_eq!(v["schema"],"s3-local-storage-fault/2");
            } assert_eq!(values[1]["injected"],true);
            assert_eq!(values[1]["phase"],if panic {"panic"}else{"scope_error"});
            assert!(!raw.contains("private"));
            assert!(recorded::<()>(&f,&p,b"exact synthetic command",||panic!("retry"),|file|file.sync_all()).is_err());
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test] fn reservation_sync_collision_permissions_and_digest_reject_before_action() {
        let p=root();let f=StorageFault::new("before_wal",1,true,true).unwrap();
        for bytes in [vec![], vec![0;16385]] {
            assert!(recorded::<()>(&f,&p,&bytes,||panic!("action"),|file|file.sync_all()).is_err());
        }
        std::fs::set_permissions(&p,std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(recorded::<()>(&f,&p,b"exact synthetic command",||panic!("action"),|file|file.sync_all()).is_err());
        std::fs::set_permissions(&p,std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(recorded::<()>(&f,&p,b"exact synthetic command",||panic!("action"),|_|Err(std::io::Error::other("sync"))).is_err());
        assert!(p.join("storage-fault.jsonl").exists());
        assert!(recorded::<()>(&f,&p,b"exact synthetic command",||panic!("action"),|file|file.sync_all()).is_err());
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test] fn final_sync_failure_and_path_replacement_never_report_success() {
        for replace in [false,true] {
            let p=root(); let moved=p.with_extension("moved");
            let f=StorageFault::new("before_wal",1,true,true).unwrap();let count=Cell::new(0);
            assert!(recorded(&f,&p,b"another synthetic command",|| {
                if replace {std::fs::rename(&p,&moved).unwrap();std::fs::create_dir(&p).unwrap();}
                Ok(())
            },|file|{count.set(count.get()+1);if !replace && count.get()==3 {Err(std::io::Error::other("final sync"))}else{file.sync_all()}}).is_err());
            assert!(f.hook()("before_wal").is_err());
            std::fs::remove_dir_all(&p).unwrap();if replace{std::fs::remove_dir_all(moved).unwrap();}
        }
    }
}

#[cfg(test)]
mod errno_tests {
    use super::*;
    #[test] fn exact_host_errno_at_selected_visit_then_closed() {
        for (mode, errno) in [(IoFault::Enospc,libc::ENOSPC),
            (IoFault::Edquot,libc::EDQUOT),(IoFault::Eio,libc::EIO)] {
            let f=StorageFault::with_io_fault("wal_sync",2,mode,true,true).unwrap();
            let h=f.hook(); h("before_wal").unwrap(); h("wal_sync").unwrap();
            match h("wal_sync") {Err(Error::Io(e))=>assert_eq!(e.raw_os_error(),Some(errno)),
                _=>panic!("wrong fault")}
            assert!(matches!(h("wal_sync"),Err(Error::Recovery("STORAGE_FAULT_CLOSED"))));
            assert_eq!(f.finish().unwrap(),Report{visits:3,matching_visits:2,injected:true});
        }
        match StorageFault::new("wal_sync",1,true,true).unwrap().hook()("wal_sync") {
            Err(Error::Io(e))=>assert_eq!(e.raw_os_error(),None), _=>panic!("generic changed") }
    }
    #[test] fn errno_modes_require_both_optins_and_exact_point() {
        for mode in [IoFault::Enospc,IoFault::Edquot,IoFault::Eio] {
            for (p,n,a,b) in [("wal_sync",1,false,true),("wal_sync",1,true,false),
                ("ENOSPC",1,true,true),("wal_sync",0,true,true),("wal_sync",1025,true,true)] {
                assert!(StorageFault::with_io_fault(p,n,mode,a,b).is_err());
            }
            let f=StorageFault::with_io_fault("wal_sync",1,mode,true,true).unwrap();
            f.hook()("before_wal").unwrap(); assert!(!f.finish().unwrap().injected);
        }
    }
    #[test] fn errno_report_binds_command_and_python_reader() {
        use std::os::unix::fs::PermissionsExt;
        for mode in [IoFault::Enospc,IoFault::Edquot,IoFault::Eio] {
            let p=std::env::temp_dir().canonicalize().unwrap().join(format!("errno-record-{}-{}",std::process::id(),mode.label()));
            std::fs::create_dir(&p).unwrap();
            std::fs::set_permissions(&p,std::fs::Permissions::from_mode(0o700)).unwrap();
            let f=StorageFault::with_io_fault("wal_sync",1,mode,true,true).unwrap();
            let value=serde_json::json!({"command":"synthetic hook", "point":"wal_sync",
                "occurrence":"1", "io_fault":mode.label()});
            let raw=serde_json::to_vec(&value).unwrap();
            for field in ["io_fault","point","occurrence"] {
                let mut bad=value.clone(); bad[field]=serde_json::json!("wrong");
                assert!(recorded::<()>(&f,&p,&serde_json::to_vec(&bad).unwrap(),||panic!("effect"),|_|panic!("sync")).is_err());
                assert_eq!(std::fs::read_dir(&p).unwrap().count(),0);
            }
            let duplicate=String::from_utf8(raw.clone()).unwrap().replacen("{","{\"io_fault\":\"EIO\",",1);
            assert!(recorded::<()>(&f,&p,duplicate.as_bytes(),||panic!("effect"),|_|panic!("sync")).is_err());
            recorded(&f,&p,&raw,|| {
                match f.hook()("wal_sync") { Err(Error::Io(e))=>assert_eq!(e.raw_os_error(),mode.error().raw_os_error()), _=>panic!("not injected") }
                Ok(())
            },|file|file.sync_all()).unwrap();
            let out=std::process::Command::new("python3")
                .arg(std::path::Path::new(file!()).parent().unwrap().parent().unwrap().join("check_real_fault_report.py"))
                .arg(&p).arg(nus_exchange_contract::s3::journal::sha256(&raw)).output().unwrap();
            assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
            let lines=std::fs::read_to_string(p.join("storage-fault.jsonl")).unwrap();
            for line in lines.lines() {
                let r:serde_json::Value=serde_json::from_str(line).unwrap();
                assert_eq!(r["io_fault"],mode.label());assert_eq!(r["schema"],"s3-local-storage-fault/3");
            }
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test] fn unbound_errno_record_rejected_before_filesystem_or_action() {
        for mode in [IoFault::Enospc,IoFault::Edquot,IoFault::Eio] {
            let f=StorageFault::with_io_fault("wal_sync",1,mode,true,true).unwrap();
            let r:Result<()>=recorded(&f,std::path::Path::new("relative-invalid-root"),b"command",
                ||panic!("action executed"), |_|panic!("sync executed"));
            assert!(matches!(r,Err(Error::Invalid("FAULT_REPORT_ERRNO_UNBOUND"))));
            assert_eq!(f.finish().unwrap(),Report{visits:0,matching_visits:0,injected:false});
        }
    }
}

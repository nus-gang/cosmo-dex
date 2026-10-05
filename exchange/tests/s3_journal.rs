use nus_exchange_contract::s3::journal::{self, CrashPoint, Error, Journal, MAX_PAYLOAD};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Self(root.join(format!(
            "s3-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn context() -> Value {
    json!({"service_schema":"s3/1", "chain_id":"nus-s3-dev-1", "genesis_hash":"01".repeat(32), "contract_hash":"02".repeat(32), "config_hash":"03".repeat(32), "market_id":"DEVBASE/DEVQUOTE", "market_config_version":"1"})
}
fn record(j: &Journal) -> Value {
    json!({"context":context(), "command_seq":(j.commit().command_seq+1).to_string(),
      "previous_commit_hash":j.commit().record_hash, "command_kind":"ORDER",
      "outbox":[{"fill_id":"synthetic-fill", "submission_enabled":true}], "result":"fixture-local", "evidence_refs":[]})
}
fn run_child(dir: &Dir, mode: &str) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_process", "--nocapture"])
        .env("S3_JOURNAL_CHILD_DIR", &dir.0)
        .env("S3_JOURNAL_CHILD_MODE", mode)
        .status()
        .unwrap()
}
#[test]
fn child_process() {
    let Some(path) = std::env::var_os("S3_JOURNAL_CHILD_DIR") else {
        return;
    };
    let mode = std::env::var("S3_JOURNAL_CHILD_MODE").unwrap();
    let opened = Journal::open(&PathBuf::from(path), context());
    if mode == "lock" {
        assert!(matches!(opened, Err(Error::WriterAlreadyRunning)));
        return;
    }
    let (mut j, _) = opened.unwrap();
    let point = match mode.as_str() {
        "before" => CrashPoint::BeforeAppend,
        "wal" => CrashPoint::AfterWalSync,
        "partial" => CrashPoint::DuringWalWrite,
        "marker" => CrashPoint::AfterMarkerSync,
        "rename" => CrashPoint::AfterRename,
        "commit" => CrashPoint::AfterCommit,
        _ => panic!("unknown crash point"),
    };
    j.append_with_crash(&record(&j), 4096, false, Some(point))
        .unwrap();
    panic!("crash hook failed");
}
#[test]
fn single_writer_is_enforced_across_processes() {
    let d = Dir::new();
    let j = Journal::create(&d.0, context()).unwrap();
    assert!(run_child(&d, "lock").success());
    drop(j);
    assert!(Journal::open(&d.0, context()).is_ok());
}
#[test]
fn committed_record_and_outbox_recover_atomically() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let r = record(&j);
    let ack = j.append(&r, 4096, false).unwrap();
    drop(j);
    let (j, records) = Journal::open(&d.0, context()).unwrap();
    assert_eq!(j.commit(), &ack);
    assert_eq!(records, vec![r]);
    assert_eq!(records[0]["outbox"][0]["submission_enabled"], true);
}

#[cfg(unix)]
#[test]
fn dropped_writer_reopens_while_unrelated_child_is_before_exec() {
    use std::{
        ffi::{c_int, c_void},
        io::{Read, Write},
        os::unix::{io::AsRawFd, net::UnixStream, process::CommandExt},
        time::Duration,
    };
    // POSIX read/write signatures, confined to this Unix test. No dependency or
    // protocol-pinned Cargo.lock change is needed for the two pre-exec syscalls.
    unsafe extern "C" {
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    }

    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let r = record(&j);
    let ack = j.append(&r, 4096, false).unwrap();
    let (mut ready, child_ready) = UnixStream::pair().unwrap();
    let (mut release, child_release) = UnixStream::pair().unwrap();
    let timeout = Some(Duration::from_secs(10));
    ready.set_read_timeout(timeout).unwrap();
    release.set_write_timeout(timeout).unwrap();
    child_release.set_read_timeout(timeout).unwrap();
    child_ready.set_write_timeout(timeout).unwrap();

    // A pre_exec hook forces fork, then holds the child before CLOEXEC closes
    // inherited descriptors. No sleeps or probabilistic scheduling are needed.
    let child = std::thread::spawn(move || {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "child_process"])
            .env_remove("S3_JOURNAL_CHILD_DIR");
        // SAFETY: the hook only performs async-signal-safe syscalls on sockets
        // created above. No Journal, allocator, assertion, or Rust lock is used.
        unsafe {
            command.pre_exec(move || {
                let mut byte = 1u8;
                if write(child_ready.as_raw_fd(), (&byte as *const u8).cast(), 1) != 1
                    || read(child_release.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) != 1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.status()
    });
    let ready_result = ready.read_exact(&mut [0]);
    let original_excludes_writer = matches!(
        Journal::open(&d.0, context()),
        Err(Error::WriterAlreadyRunning)
    );
    drop(j);
    let reopened = Journal::open(&d.0, context());
    let replacement_excludes_writer = matches!(
        Journal::open(&d.0, context()),
        Err(Error::WriterAlreadyRunning)
    );
    // Release and reap even on the expected pre-fix error, before asserting.
    let release_result = release.write_all(&[1]);
    let child_status = child.join().unwrap().unwrap();
    ready_result.unwrap();
    release_result.unwrap();
    assert!(child_status.success());
    assert!(original_excludes_writer);
    let (mut j, records) = reopened.unwrap();
    assert_eq!(j.commit(), &ack);
    assert_eq!(records, vec![r]);
    assert!(replacement_excludes_writer);
    // Closing the old child's descriptors must not release the new owner's lock.
    assert!(matches!(
        Journal::open(&d.0, context()),
        Err(Error::WriterAlreadyRunning)
    ));
    assert!(run_child(&d, "lock").success());
    j.append(&record(&j), 4096, false).unwrap();
    drop(j);
    assert_eq!(Journal::open(&d.0, context()).unwrap().1.len(), 2);
}

#[test]
fn crash_points_never_silently_lose_a_committed_record() {
    for repetition in 0..3 {
        for point in ["before", "partial", "wal", "marker", "rename", "commit"] {
            let d = Dir::new();
            let j = Journal::create(&d.0, context()).unwrap();
            drop(j);
            assert_eq!(run_child(&d, point).code(), Some(86));
            let original = fs::read(d.0.join("journal.wal")).unwrap();
            for replay in 0..2 {
                let result = Journal::open(&d.0, context());
                match point {
                    "before" => assert_eq!(result.unwrap().1.len(), 0),
                    "partial" | "wal" | "marker" => {
                        assert!(matches!(result, Err(Error::RecoveryRequired(_))));
                        assert_eq!(fs::read(d.0.join("journal.wal")).unwrap(), original);
                        assert!(fs::read_dir(&d.0).unwrap().any(|e| {
                            e.unwrap()
                                .file_name()
                                .to_string_lossy()
                                .starts_with("evidence-")
                        }));
                    }
                    // Process termination after rename is not a power-loss test.
                    "rename" | "commit" => assert_eq!(result.unwrap().1.len(), 1),
                    _ => unreachable!(),
                }
                println!(
                    "S3_EVIDENCE {}",
                    json!({"case":"storage-crash", "point":point, "repetition":repetition.to_string(), "replay":replay.to_string(), "expected":if matches!(point,"partial"|"wal"|"marker") {"RECOVERY_REQUIRED_EVIDENCE_PRESERVED"} else if point=="before" {"ZERO_COMMIT"} else {"ONE_COMMIT"}, "source":"REAL_FILESYSTEM_PROCESS_EXIT_NOT_POWER_LOSS", "expected_diff":[], "result":"PASS"})
                );
            }
        }
    }
}
#[test]
fn completed_frame_loss_header_payload_marker_corruption_stop_recovery() {
    for kind in [
        "missing",
        "header",
        "payload",
        "partial",
        "marker",
        "unknown-tail",
    ] {
        let d = Dir::new();
        let mut j = Journal::create(&d.0, context()).unwrap();
        j.append(&record(&j), 4096, false).unwrap();
        drop(j);
        let wal = d.0.join("journal.wal");
        let mut bytes = fs::read(&wal).unwrap();
        match kind {
            "missing" => bytes.clear(),
            "header" => bytes[4] ^= 1,
            "payload" => bytes[72] ^= 1,
            "partial" => {
                bytes.pop();
            }
            "marker" => fs::write(d.0.join("commit.marker"), b"bad").unwrap(),
            "unknown-tail" => bytes.extend(b"S3"),
            _ => unreachable!(),
        }
        fs::write(&wal, &bytes).unwrap();
        assert!(Journal::open(&d.0, context()).is_err(), "{kind}");
        assert_eq!(fs::read(wal).unwrap(), bytes);
    }
}
#[test]
fn resource_limit_has_no_sequence_or_wal_effect() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let before = j.commit().clone();
    assert!(matches!(
        j.append(&record(&j), MAX_PAYLOAD + 1, false),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(j.commit(), &before);
    assert_eq!(fs::metadata(d.0.join("journal.wal")).unwrap().len(), 0);
    j.append(&record(&j), MAX_PAYLOAD, false).unwrap();
}
#[test]
fn correction_consumes_and_replenishes_reserve() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    j.append(&record(&j), 4096, false).unwrap();
    let before = fs::metadata(d.0.join("correction.reserve")).unwrap().len();
    let mut correction = record(&j);
    correction["command_kind"] = json!("CORRECTION");
    j.append(&correction, 4096, true).unwrap();
    assert_eq!(
        fs::metadata(d.0.join("correction.reserve")).unwrap().len(),
        before
    );
    drop(j);
    assert_eq!(Journal::open(&d.0, context()).unwrap().1.len(), 2);
}
#[test]
fn malformed_records_context_change_and_existing_namespace_are_rejected() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    assert!(Journal::create(&d.0, context()).is_err());
    let mut r = record(&j);
    r["command_seq"] = json!("01");
    assert!(j.append(&r, 4096, false).is_err());
    r = record(&j);
    r["number"] = json!(1);
    assert!(j.append(&r, 4096, false).is_err());
    r = record(&j);
    r["unicode"] = json!("한글");
    assert!(j.append(&r, 4096, false).is_err());
    drop(j);
    assert!(Journal::open(&d.0, json!({"wrong":"context"})).is_err());
}

#[test]
fn content_addressed_raw_evidence_is_required_on_append_and_every_replay() {
    for damage in ["missing", "truncated", "same-size-corruption", "none"] {
        let d = Dir::new();
        let mut j = Journal::create(&d.0, context()).unwrap();
        let raw = b"{\"fixture\":\"synthetic RPC bytes, not real chain proof\"}";
        let reference = j.store_evidence(raw, "application/json").unwrap();
        assert_eq!(
            j.store_evidence(raw, "application/json").unwrap(),
            reference
        );
        let object =
            d.0.join("objects")
                .join(reference["sha256"].as_str().unwrap());
        assert_eq!(fs::read(&object).unwrap(), raw);
        let mut r = record(&j);
        r["evidence_refs"] = json!([reference]);
        let ack = j.append(&r, 4096, false).unwrap();
        drop(j);
        match damage {
            "missing" => fs::remove_file(object).unwrap(),
            "truncated" => fs::write(object, b"x").unwrap(),
            "same-size-corruption" => fs::write(object, vec![b'x'; raw.len()]).unwrap(),
            _ => (),
        }
        let wal = fs::read(d.0.join("journal.wal")).unwrap();
        for _ in 0..2 {
            let opened = Journal::open(&d.0, context());
            if damage == "none" {
                let (j, records) = opened.unwrap();
                assert_eq!(j.commit(), &ack);
                assert_eq!(records, vec![r.clone()]);
            } else {
                assert!(matches!(opened, Err(Error::RecoveryRequired(_))));
            }
            assert_eq!(fs::read(d.0.join("journal.wal")).unwrap(), wal);
        }
    }
}

#[test]
fn missing_or_traversal_evidence_cannot_be_committed() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let before = j.commit().clone();
    for hash in ["a".repeat(64), "../context.json".into()] {
        let mut r = record(&j);
        r["evidence_refs"] =
            json!([{"sha256":hash,"byte_length":"1","media_type":"application/json"}]);
        assert!(j.append(&r, 4096, false).is_err());
        assert_eq!(j.commit(), &before);
        assert_eq!(fs::metadata(d.0.join("journal.wal")).unwrap().len(), 0);
    }
}

#[test]
fn s3_magic_and_namespace_reject_s2_without_modifying_old_home() {
    use nus_exchange_contract::s2::journal as s2;
    let d = Dir::new();
    let old_context = json!({"schema_version":"1","genesis_hash":"S2 synthetic fixture"});
    let j = s2::Journal::create(&d.0, old_context.clone()).unwrap();
    drop(j);
    let files = || {
        let mut files: Vec<_> = fs::read_dir(&d.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        files.sort();
        files
    };
    let before = files();
    assert!(Journal::open(&d.0, context()).is_err());
    assert_eq!(files(), before);
    assert!(Journal::create(&Dir::new().0, old_context).is_err());
    let payload = b"{\"schema\":\"s3/1\"}";
    let frame = journal::frame(payload).unwrap();
    assert_eq!(&frame[..4], b"S3W1");
    assert_eq!(frame.len(), 72 + payload.len());
    assert_ne!(frame, s2::frame(payload).unwrap());
    // Swapping a valid checksummed S2 frame into S3 must not pass replay.
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let r = record(&j);
    j.append(&r, 4096, false).unwrap();
    drop(j);
    fs::write(
        d.0.join("journal.wal"),
        s2::frame(&journal::canonical(&r).unwrap()).unwrap(),
    )
    .unwrap();
    assert!(Journal::open(&d.0, context()).is_err());
}

#[test]
fn payload_limit_exact_16mib_boundary() {
    let exact = vec![b'x'; MAX_PAYLOAD];
    assert_eq!(journal::frame(&exact).unwrap().len(), MAX_PAYLOAD + 72);
    assert!(matches!(
        journal::frame(&vec![b'x'; MAX_PAYLOAD + 1]),
        Err(Error::ResourceLimit)
    ));
}

#[test]
fn correction_flag_cannot_spend_reserve_for_an_ordinary_command() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let before = j.commit().clone();
    assert!(matches!(
        j.append(&record(&j), 4096, true),
        Err(Error::InvalidRecord("CORRECTION_RESERVE_ACCESS"))
    ));
    assert_eq!(j.commit(), &before);
}

#[test]
fn status_revision_reservations_survive_restart_and_fail_closed() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    assert!(j.reserve_status_revisions(0).is_err());
    assert_eq!(j.reserve_status_revisions(100).unwrap(), 1..=100);
    let commit = j.commit().clone();
    drop(j); // only the first revision might have been published
    let (mut j, records) = Journal::open(&d.0, context()).unwrap();
    assert!(records.is_empty());
    assert_eq!(j.reserve_status_revisions(10).unwrap(), 101..=110);
    assert_eq!(j.commit(), &commit);
    let original = fs::read(d.0.join("status.revision")).unwrap();
    fs::write(d.0.join("status.revision.tmp"), b"interrupted").unwrap();
    assert!(j.reserve_status_revisions(10).is_err());
    assert_eq!(fs::read(d.0.join("status.revision")).unwrap(), original);
    assert!(j.reserve_status_revisions(10).is_err());
}

#[test]
fn status_revision_missing_corrupt_or_exhausted_never_resets() {
    for mode in ["missing", "corrupt", "exhausted", "binding"] {
        let d = Dir::new();
        let mut j = Journal::create(&d.0, context()).unwrap();
        let path = d.0.join("status.revision");
        match mode {
            "missing" => fs::remove_file(&path).unwrap(),
            "corrupt" => fs::write(&path, b"S3W1").unwrap(),
            _ => {
                let v = json!({"context":if mode == "binding" {json!({})} else {context()},
                    "reserved_through":u64::MAX.to_string()});
                fs::write(
                    &path,
                    journal::frame(&journal::canonical(&v).unwrap()).unwrap(),
                )
                .unwrap();
            }
        }
        assert!(j.reserve_status_revisions(1).is_err(), "{mode}");
        assert!(j.reserve_status_revisions(1).is_err(), "{mode}");
    }
}

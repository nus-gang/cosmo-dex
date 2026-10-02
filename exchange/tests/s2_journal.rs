use nus_exchange_contract::s2::journal::{self, CrashPoint, Error, Journal, MAX_PAYLOAD};
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
            "s2-journal-{}-{}",
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
    json!({"genesis_hash":"fixture-only", "contract_hash":"rc2-test"})
}
fn record(j: &Journal) -> Value {
    json!({"context":context(), "command_seq":(j.commit().command_seq+1).to_string(),
      "previous_commit_hash":j.commit().record_hash, "command_kind":"ORDER",
      "outbox":[{"fill_id":"synthetic-fill", "submission_enabled":false}], "result":"fixture-local"})
}
fn run_child(dir: &Dir, mode: &str) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_process", "--nocapture"])
        .env("S2_JOURNAL_CHILD_DIR", &dir.0)
        .env("S2_JOURNAL_CHILD_MODE", mode)
        .status()
        .unwrap()
}
#[test]
fn child_process() {
    let Some(path) = std::env::var_os("S2_JOURNAL_CHILD_DIR") else {
        return;
    };
    let mode = std::env::var("S2_JOURNAL_CHILD_MODE").unwrap();
    let opened = Journal::open(&PathBuf::from(path), context());
    if mode == "lock" {
        assert!(matches!(opened, Err(Error::WriterAlreadyRunning)));
        return;
    }
    let (mut j, _) = opened.unwrap();
    let point = match mode.as_str() {
        "before" => CrashPoint::BeforeAppend,
        "wal" => CrashPoint::AfterWalSync,
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
fn framing_matches_approved_fixture() {
    let v: Value =
        serde_json::from_str(include_str!("../../protocol/s2/vectors/wal.json")).unwrap();
    let payload = hex::decode(v["payload_hex"].as_str().unwrap()).unwrap();
    let frame = journal::frame(&payload).unwrap();
    assert_eq!(hex::encode(&frame), v["frame_hex"]);
    assert_eq!(journal::sha256(&frame), v["record_hash"]);
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
    assert_eq!(records[0]["outbox"][0]["submission_enabled"], false);
}
#[test]
fn crash_points_never_silently_lose_a_committed_record() {
    for point in ["before", "wal", "marker", "rename", "commit"] {
        let d = Dir::new();
        let j = Journal::create(&d.0, context()).unwrap();
        drop(j);
        assert_eq!(run_child(&d, point).code(), Some(86));
        let original = fs::read(d.0.join("journal.wal")).unwrap();
        let result = Journal::open(&d.0, context());
        match point {
            "before" => assert_eq!(result.unwrap().1.len(), 0),
            "wal" | "marker" => {
                assert!(matches!(result, Err(Error::RecoveryRequired(_))));
                assert_eq!(fs::read(d.0.join("journal.wal")).unwrap(), original);
                assert!(fs::read_dir(&d.0).unwrap().any(|e| {
                    e.unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with("evidence-")
                }));
            }
            // Process termination after rename is not a power-loss simulation.
            "rename" | "commit" => assert_eq!(result.unwrap().1.len(), 1),
            _ => unreachable!(),
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
            "unknown-tail" => bytes.extend(b"S2"),
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
fn authenticated_s2_order_bytes_survive_process_restart() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use nus_exchange_contract::policy::{OrderContext, authenticate_order};
    let vectors: Value =
        serde_json::from_str(include_str!("../../protocol/s2/vectors/signed.json")).unwrap();
    let v = &vectors["cases"][0];
    let fields = &v["fields"];
    let wire = hex::decode(v["canonical_hex"].as_str().unwrap()).unwrap();
    let sig = hex::decode(v["signature_hex"].as_str().unwrap()).unwrap();
    // Registered-key input is fixed trusted test evidence, not a client-selected key.
    let registered = hex::decode(v["public_key_hex"].as_str().unwrap()).unwrap();
    let ctx = OrderContext {
        snapshot_id: "fixture",
        chain_id: fields[1][2].as_str().unwrap(),
        genesis_hash: fields[2][2].as_str().unwrap(),
        exchange_module_id: "x/exchange",
        market_id: "DEVBASE/DEVQUOTE",
        market_config_version: 1,
        registered_key_type: Some("ML-DSA-65"),
        registered_key: Some(&registered),
        epoch: 0,
        height: 100,
        revoked: false,
        filled: 0,
        available: 0,
        fee_bps: 0,
    };
    authenticate_order(&wire, &sig, &ctx).unwrap();
    let mut bad = sig.clone();
    bad[0] ^= 1;
    assert!(authenticate_order(&wire, &bad, &ctx).is_err());
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let mut r = record(&j);
    r["request_wire"] = json!(STANDARD.encode(&wire));
    r["signature"] = json!(STANDARD.encode(&sig));
    r["signature_hash"] = json!(journal::sha256(&sig));
    let ack = j.append(&r, 20000, false).unwrap();
    drop(j);
    // A separate process acquires and releases the writer before this reload.
    // The crash-before-append path makes no new logical command.
    assert_eq!(run_child(&d, "before").code(), Some(86));
    let (j, restored) = Journal::open(&d.0, context()).unwrap();
    assert_eq!(j.commit(), &ack);
    assert_eq!(restored, vec![r]);
    let restored_wire = STANDARD
        .decode(restored[0]["request_wire"].as_str().unwrap())
        .unwrap();
    let restored_sig = STANDARD
        .decode(restored[0]["signature"].as_str().unwrap())
        .unwrap();
    authenticate_order(&restored_wire, &restored_sig, &ctx).unwrap();
}

#[test]
fn marker_write_failure_poisoning_preserves_unknown_tail() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    // Deterministic actual filesystem error after WAL fsync, without mocking IO.
    fs::create_dir(d.0.join("marker.tmp")).unwrap();
    assert!(matches!(
        j.append(&record(&j), 4096, false),
        Err(Error::Io(_))
    ));
    assert!(matches!(
        j.append(&record(&j), 4096, false),
        Err(Error::RecoveryRequired("POISONED_WRITER"))
    ));
    assert_eq!(j.commit().command_seq, 0);
    assert!(fs::metadata(d.0.join("journal.wal")).unwrap().len() > 0);
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
            "corrupt" => fs::write(&path, b"S2W1").unwrap(),
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

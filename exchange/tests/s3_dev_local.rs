#![cfg(feature = "dev-local-demo")]
#[path = "support/dev_fixture.rs"]
mod fixture;
use fixture::*;
use nus_exchange_contract::s3::{
    dev_local::{Command, Engine, Validated},
    journal::{canonical, sha256},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command as Process,
    sync::{Arc, mpsc},
};
fn new(
    bps: u32,
) -> (
    std::path::PathBuf,
    nus_exchange_contract::s3::dev_local::Inputs,
    Value,
    Engine,
) {
    let (i, v) = initial(bps);
    let h = home(bps);
    let e = Engine::create(
        &h,
        Validated::new(i.clone()).unwrap(),
        &canonical(&v).unwrap(),
    )
    .unwrap();
    (h, i, v, e)
}
#[test]
fn manifest_profile_guard_rejections() {
    let (i, _) = initial(0);
    let mut results = vec![];
    for field in [
        "opt_in",
        "profile",
        "runtime_pin",
        "guard_extra",
        "guard_duplicate",
        "genesis",
        "inherited",
        "descriptor_resealed",
        "component_scope",
    ] {
        let mut j = i.clone();
        match field {
            "opt_in" => j.acknowledge_unproven_space = false,
            "profile" => j.effective_profile = b"{}".to_vec(),
            "runtime_pin" => j.approved_runtime_sha256 = "0".repeat(64),
            "guard_extra" => {
                let mut v: Value = serde_json::from_slice(&j.guard).unwrap();
                v["extra"] = json!("x");
                j.guard = canonical(&v).unwrap();
            }
            "guard_duplicate" => {
                j.guard
                    .splice(1..1, b"\"profile_id\":\"s3-dev-local-v1\",".iter().cloned());
            }
            "genesis" => j.genesis.push(b' '),
            "inherited" => {
                j.files
                    .get_mut("protocol/s3/CONTRACT.md")
                    .unwrap()
                    .push(b' ');
            }
            "descriptor_resealed" => {
                j.files
                    .get_mut("chain/local-demo/components/exchange.json")
                    .unwrap()
                    .push(b' ');
                let mut m: Value = serde_json::from_slice(&j.runtime_manifest).unwrap();
                m["files_sha256"]["chain/local-demo/components/exchange.json"] = json!(sha256(
                    &j.files["chain/local-demo/components/exchange.json"]
                ));
                m["contract_sha256"] = json!(aggregate(&j.files));
                j.runtime_manifest = canonical(&m).unwrap();
            }
            "component_scope" => {
                let mut m: Value = serde_json::from_slice(&j.runtime_manifest).unwrap();
                m["scope"] = json!("COMPONENT_FIXTURE");
                j.runtime_manifest = canonical(&m).unwrap();
                j.approved_runtime_sha256 = sha256(&j.runtime_manifest);
            }
            _ => unreachable!(),
        };
        let err = Validated::new(j).err().expect(field);
        results.push(json!({"case":field,"error":err.to_string(),"result":"PASS"}));
    }
    evidence("input-gates", &json!(results));
}
#[test]
fn committed_receipts_duplicates_and_two_replays() {
    for bps in [0, 25] {
        let (h, i, v, e) = new(bps);
        let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 71);
        let o = observation(&v);
        let receipt = e
            .execute(signed(&raw, &sig, 0), &[], &o, NOW)
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt["development_receipt"],
            "LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE"
        );
        assert_eq!(receipt["durable_ack"], false);
        assert_eq!(receipt["command_result"]["code"], "OK");
        let before = e.reader().get().unwrap();
        assert_eq!(
            e.execute(signed(&raw, &sig, 0), &[], &o, NOW).unwrap(),
            Some(receipt.clone())
        );
        assert_eq!(before.commit, e.reader().get().unwrap().commit);
        assert!(
            e.query_signed("ORDER", &raw, &sig, &owner(1), &o, NOW)
                .is_err()
        );
        let ledger = before.receipts.values().cloned().collect::<Vec<_>>();
        drop(e);
        for _ in 0..2 {
            let e = Engine::open(&h, Validated::new(i.clone()).unwrap()).unwrap();
            let view = e.reader().get().unwrap();
            assert_eq!(view.state, before.state);
            assert_eq!(view.commit, before.commit);
            assert_eq!(view.receipts, before.receipts);
            assert_eq!(
                e.query_signed("ORDER", &raw, &sig, &owner(0), &o, NOW)
                    .unwrap(),
                Some(receipt.clone())
            );
            e.reconcile_receipt_ledger(&ledger).unwrap();
            let mut bad = ledger.clone();
            bad[0]["record_hash"] = json!("0".repeat(64));
            assert!(e.reconcile_receipt_ledger(&bad).is_err());
        }
        copy_home(&format!("receipt-fee{bps}"), &h);
        evidence(
            &format!("receipts-fee{bps}"),
            &json!({"scope":"COMPONENT_SYNTHETIC_DESCRIPTOR_NOT_APPROVED_RUNTIME","input_bundle":bundle(&i),"effective_profile_base64":base64::Engine::encode(&base64::engine::general_purpose::STANDARD,&i.effective_profile),"runtime_test_pin":i.approved_runtime_sha256,"bootstrap":v,"signed_request_hex":hex::encode(raw),"signature_hex":hex::encode(sig),"receipt_ledger":ledger,"state":before.state,"replay_diff":[],"result":"PASS"}),
        );
    }
}
#[test]
fn home_link_and_cross_store_rejections() {
    let mut results = vec![];
    for case in [
        "root_mode",
        "root_symlink",
        "root_copy",
        "guard_symlink",
        "guard_hardlink",
        "wal_hardlink",
        "marker_symlink",
        "raw_partial",
        "missing_guard",
        "standard_files",
    ] {
        let (h, i, _, e) = new(0);
        drop(e);
        let original = fs::read(h.join("profile.guard.json")).unwrap();
        let outside = h.parent().unwrap().join(format!("outside-{case}"));
        match case {
            "root_mode" => fs::set_permissions(&h, fs::Permissions::from_mode(0o755)).unwrap(),
            "root_symlink" => {
                fs::rename(&h, &outside).unwrap();
                symlink(&outside, &h).unwrap();
            }
            "root_copy" => {
                fs::rename(&h, &outside).unwrap();
                fn copy(a: &std::path::Path, b: &std::path::Path) {
                    fs::create_dir(b).unwrap();
                    fs::set_permissions(b, fs::Permissions::from_mode(0o700)).unwrap();
                    for e in fs::read_dir(a).unwrap() {
                        let e = e.unwrap();
                        if e.file_type().unwrap().is_dir() {
                            copy(&e.path(), &b.join(e.file_name()));
                        } else {
                            fs::copy(e.path(), b.join(e.file_name())).unwrap();
                        }
                    }
                }
                copy(&outside, &h);
            }
            "guard_symlink" | "marker_symlink" => {
                let n = if case == "guard_symlink" {
                    "profile.guard.json"
                } else {
                    "commit.dev.json"
                };
                fs::rename(h.join(n), &outside).unwrap();
                symlink(&outside, h.join(n)).unwrap();
            }
            "guard_hardlink" | "wal_hardlink" => {
                fs::hard_link(
                    h.join(if case == "guard_hardlink" {
                        "profile.guard.json"
                    } else {
                        "journal.dev.wal"
                    }),
                    &outside,
                )
                .unwrap();
            }
            "raw_partial" => fs::write(
                h.join("objects/sha256")
                    .join(format!("{}.tmp", "f".repeat(64))),
                b"partial",
            )
            .unwrap(),
            "missing_guard" => {
                fs::rename(h.join("profile.guard.json"), &outside).unwrap();
            }
            "standard_files" => fs::write(h.join("journal.wal"), b"S3W1").unwrap(),
            _ => unreachable!(),
        }
        let error = Engine::open(&h, Validated::new(i.clone()).unwrap())
            .err()
            .expect(case)
            .to_string();
        assert!(Engine::create(&h, Validated::new(i).unwrap(), b"{}").is_err());
        if case != "missing_guard" {
            assert_eq!(fs::read(h.join("profile.guard.json")).unwrap(), original);
        }
        results.push(json!({"case":case,"error":error,"result":"PASS"}));
    }
    let (h, i, _, e) = new(0);
    let ctx = Validated::new(i).unwrap().context().clone();
    drop(e);
    assert!(nus_exchange_contract::s3::journal::Journal::open(&h, ctx.clone()).is_err());
    let h2 = home(0);
    let j = nus_exchange_contract::s3::journal::Journal::create(&h2, ctx).unwrap();
    drop(j);
    let (i, _) = initial(0);
    assert!(Engine::open(&h2, Validated::new(i).unwrap()).is_err());
    evidence("home-gates", &json!(results));
}
#[cfg(feature = "fault-injection")]
#[test]
fn io_errors_close_admission_and_preserve_unknown() {
    let mut results = vec![];
    for point in [
        "file_sync",
        "publish_dir_sync",
        "before_wal",
        "partial_wal",
        "wal_sync",
        "after_wal_sync",
        "marker_sync",
        "after_marker_sync",
        "after_marker_rename",
        "marker_dir_sync",
        "after_marker_dir_sync",
        "after_commit",
        "before_publish",
        "before_response",
    ] {
        let (h, i, v, e) = new(0);
        let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 72);
        let o = observation(&v);
        let p = point.to_owned();
        e.set_fault_hook(Some(Arc::new(move |at| {
            if at == p {
                Err(std::io::Error::from_raw_os_error(5).into())
            } else {
                Ok(())
            }
        })))
        .unwrap();
        assert!(
            e.execute(signed(&raw, &sig, 0), &[], &o, NOW).is_err(),
            "{point}"
        );
        assert_eq!(e.reader().get().unwrap().gate, "RECOVERY_REQUIRED");
        assert!(e.execute(signed(&raw, &sig, 0), &[], &o, NOW).is_err());
        assert!(e.committed_attempt("x").is_err());
        let commit = e.reader().get().unwrap().commit.clone();
        assert!(e.trusted_recovery_history(&commit, None, 64).is_err());
        assert!(e.trusted_recovery_attempt_at(&commit, 0).is_err());
        assert!(
            e.trusted_recovery_attempt(&commit, nus_exchange_contract::s3::schema::ZERO)
                .is_err()
        );
        assert!(
            e.with_committed_attempt("x", |_, _| panic!("closed gate broadcast"))
                .is_err()
        );
        assert!(
            e.execute(
                Command::Local {
                    kind: "WITHDRAW_PREPARE".into(),
                    raw: canonical(&json!({"request_id":"f".repeat(64)})).unwrap(),
                    session_owner: owner(0)
                },
                &[],
                &o,
                NOW
            )
            .is_err()
        );
        let wal = fs::read(h.join("journal.dev.wal")).unwrap();
        let marker = fs::read(h.join("commit.dev.json")).unwrap();
        drop(e);
        let reopened = Engine::open(&h, Validated::new(i).unwrap());
        let committed = ["after_commit", "before_publish", "before_response"].contains(&point);
        assert_eq!(reopened.is_ok(), committed, "{point}");
        if let Ok(e) = reopened {
            assert_eq!(e.reader().get().unwrap().commit.command_seq, 1);
            assert!(
                e.execute(signed(&raw, &sig, 0), &[], &o, NOW)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(e.reader().get().unwrap().commit.command_seq, 1);
        }
        assert_eq!(wal, fs::read(h.join("journal.dev.wal")).unwrap());
        assert_eq!(marker, fs::read(h.join("commit.dev.json")).unwrap());
        copy_home(&format!("io-{point}"), &h);
        results.push(json!({"point":point,"injected_errno":"EIO(5)","scope":"SIMULATED_IO_NOT_HOST_ENOSPC","reopen":if committed{"COMPLETE_PREFIX"}else{"RECOVERY_REQUIRED"},"wal_sha256":sha256(&wal),"marker_sha256":sha256(&marker),"result":"PASS"}));
    }
    evidence("io-faults", &json!(results));
}
#[cfg(feature = "fault-injection")]
#[test]
fn readers_observe_only_complete_revisions() {
    let (_, _, v, e) = new(0);
    let e = Arc::new(e);
    let reader = e.reader();
    let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 73);
    let (tx, rx) = mpsc::channel();
    let (go, wait) = mpsc::channel();
    let wait = std::sync::Mutex::new(wait);
    e.set_fault_hook(Some(Arc::new(move |p| {
        if [
            "candidate_verified",
            "after_wal_sync",
            "after_marker_dir_sync",
            "before_publish",
            "before_response",
        ]
        .contains(&p)
        {
            tx.send(p.to_owned()).unwrap();
            wait.lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
        Ok(())
    })))
    .unwrap();
    let work = e.clone();
    let handle = std::thread::spawn(move || {
        work.execute(signed(&raw, &sig, 0), &[], &observation(&v), NOW)
            .unwrap()
    });
    let mut rows = vec![];
    for _ in 0..5 {
        let point = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let view = reader.get().unwrap();
        let expected = if point == "before_response" { 1 } else { 0 };
        assert_eq!(view.commit.command_seq, expected);
        assert_eq!(view.state["last_command_seq"], expected.to_string());
        assert_eq!(view.receipts.len(), expected as usize);
        rows.push(json!({"barrier":point,"visible_revision":expected.to_string(),"result":"PASS"}));
        go.send(()).unwrap();
    }
    assert!(handle.join().unwrap().is_some());
    evidence("reader-barriers", &json!(rows));
}
#[test]
fn raw_descriptor_and_semantic_tampering_fail_closed() {
    let mut rows = vec![];
    for case in [
        "unknown_tail",
        "S3W1_magic",
        "rehashed_semantic",
        "partial_raw",
        "raw_descriptor",
    ] {
        let (h, i, v, e) = new(0);
        let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 74);
        e.execute(signed(&raw, &sig, 0), &[], &observation(&v), NOW)
            .unwrap();
        if case == "partial_raw" || case == "raw_descriptor" {
            let mut next = v.clone();
            next["height"] = json!("101");
            finish(&mut next);
            e.execute(
                Command::Snapshot(canonical(&next).unwrap()),
                &[],
                &observation(&next),
                NOW,
            )
            .unwrap();
        }
        drop(e);
        let path = h.join("journal.dev.wal");
        let mut wal = fs::read(&path).unwrap();
        match case {
            "unknown_tail" => {
                wal.push(b'x');
                fs::write(&path, &wal).unwrap();
            }
            "S3W1_magic" => {
                wal[..4].copy_from_slice(b"S3W1");
                use sha2::Digest;
                let digest = sha2::Sha256::digest(&wal[..40]);
                wal[40..72].copy_from_slice(&digest);
                fs::write(&path, &wal).unwrap();
            }
            "rehashed_semantic" => {
                let mut r: Value = serde_json::from_slice(&wal[72..]).unwrap();
                r["after_state_hash"] = json!("f".repeat(64));
                let raw = canonical(&r).unwrap();
                use sha2::Digest;
                let mut b = b"S3D1".to_vec();
                b.extend_from_slice(&(raw.len() as u32).to_be_bytes());
                b.extend_from_slice(&sha2::Sha256::digest(&raw));
                b.extend_from_slice(&sha2::Sha256::digest(&b));
                b.extend(raw);
                fs::write(&path, &b).unwrap();
                fs::write(h.join("commit.dev.json"),canonical(&json!({"command_seq":"1","record_hash":sha256(&b),"end_offset":b.len().to_string()})).unwrap()).unwrap();
            }
            "partial_raw" | "raw_descriptor" => {
                let entry = fs::read_dir(h.join("objects/sha256"))
                    .unwrap()
                    .map(|x| x.unwrap().path())
                    .find(|p| p.extension().is_none())
                    .unwrap();
                if case == "partial_raw" {
                    fs::write(entry, b"{").unwrap();
                } else {
                    fs::write(entry.with_extension("ref"), b"{}").unwrap();
                }
            }
            _ => unreachable!(),
        }
        let preserved = fs::read(&path).unwrap();
        let error = Engine::open(&h, Validated::new(i).unwrap())
            .err()
            .expect(case)
            .to_string();
        assert_eq!(preserved, fs::read(&path).unwrap());
        rows.push(json!({"case":case,"error":error,"preserved_wal_sha256":sha256(&preserved),"result":"PASS"}));
    }
    evidence("recovery-tampering", &json!(rows));
}
#[cfg(feature = "fault-injection")]
#[test]
#[ignore]
fn child_process() {
    let mode = std::env::var("NUS70_CHILD_MODE").unwrap();
    let path = std::env::var("NUS70_CHILD_HOME").unwrap();
    let (i, v) = initial(0);
    if mode == "writer2" {
        assert!(matches!(
            Engine::open(std::path::Path::new(&path), Validated::new(i).unwrap()),
            Err(nus_exchange_contract::s3::dev_local::Error::WriterAlreadyRunning)
        ));
        return;
    }
    let e = Engine::open(std::path::Path::new(&path), Validated::new(i).unwrap()).unwrap();
    let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 75);
    e.set_fault_hook(Some(Arc::new(move |at| {
        if at == mode {
            std::process::exit(86);
        }
        Ok(())
    })))
    .unwrap();
    e.execute(signed(&raw, &sig, 0), &[], &observation(&v), NOW)
        .unwrap();
    panic!("fault missed");
}
#[cfg(feature = "fault-injection")]
#[test]
fn process_crashes_and_writer_lock() {
    let mut rows = vec![];
    for point in [
        "writer2",
        "partial_wal",
        "after_wal_sync",
        "after_marker_sync",
        "after_marker_rename",
        "after_marker_dir_sync",
        "after_commit",
        "before_response",
    ] {
        let (h, i, v, e) = new(0);
        let held = if point == "writer2" {
            Some(e)
        } else {
            drop(e);
            None
        };
        let output = Process::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_process", "--ignored", "--nocapture"])
            .env("NUS70_CHILD_MODE", point)
            .env("NUS70_CHILD_HOME", &h)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if point == "writer2" { 0 } else { 86 }),
            "{point}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop(held);
        let reopened = Engine::open(&h, Validated::new(i).unwrap());
        let complete = ["writer2", "after_commit", "before_response"].contains(&point);
        assert_eq!(reopened.is_ok(), complete, "{point}");
        if let Ok(e) = reopened {
            if point != "writer2" {
                let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 75);
                assert!(
                    e.query_signed("ORDER", &raw, &sig, &owner(0), &observation(&v), NOW)
                        .unwrap()
                        .is_some()
                );
            }
        }
        copy_home(&format!("process-{point}"), &h);
        rows.push(json!({"point":point,"exit":output.status.code().unwrap().to_string(),"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr),"result":"PASS","scope":"LOCAL_PROCESS_EXIT_NOT_POWER_LOSS"}));
    }
    evidence("process-faults", &json!(rows));
}
#[test]
fn cli_feature_options_and_standard_rejection() {
    let binary = env!("CARGO_BIN_EXE_nus-s3-local-demo");
    let mut rows = vec![];
    for args in [
        vec!["validate"],
        vec!["validate", "--local-demo-profile", "absent"],
        vec!["validate", "--acknowledge-unproven-space"],
        vec!["validate", "--unknown"],
    ] {
        let o = Process::new(binary).args(&args).output().unwrap();
        assert!(!o.status.success());
        rows.push(json!({"args":args,"stderr":String::from_utf8_lossy(&o.stderr),"exit":o.status.code().unwrap().to_string()}));
    }
    let standard = Process::new(env!("CARGO_BIN_EXE_exchange-s2"))
        .args([
            "--local-demo-profile",
            "absent",
            "--acknowledge-unproven-space",
        ])
        .output()
        .unwrap();
    assert!(!standard.status.success());
    evidence(
        "cli-rejections",
        &json!({"development":rows,"standard_exit":standard.status.code().unwrap().to_string(),"standard_stderr":String::from_utf8_lossy(&standard.stderr),"result":"PASS"}),
    );
}
#[cfg(feature = "fault-injection")]
#[test]
fn guard_is_complete_before_store_and_never_reissued() {
    let (i, v) = initial(0);
    let h = home(0);
    let inspect = h.clone();
    let expected = i.guard.clone();
    let result = Engine::create_with_fault_hook(
        &h,
        Validated::new(i.clone()).unwrap(),
        &canonical(&v).unwrap(),
        Arc::new(move |at| {
            if at == "guard_complete" {
                assert_eq!(
                    fs::read(inspect.join("profile.guard.json")).unwrap(),
                    expected
                );
                let names = fs::read_dir(&inspect)
                    .unwrap()
                    .map(|e| e.unwrap().file_name().to_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                assert_eq!(names.len(), 2);
                assert!(!inspect.join("journal.dev.wal").exists());
                return Err(std::io::Error::from_raw_os_error(5).into());
            }
            Ok(())
        }),
    );
    assert!(result.is_err());
    let guard = fs::read(h.join("profile.guard.json")).unwrap();
    assert!(Engine::open(&h, Validated::new(i.clone()).unwrap()).is_err());
    assert!(Engine::create(&h, Validated::new(i).unwrap(), &canonical(&v).unwrap()).is_err());
    assert_eq!(guard, fs::read(h.join("profile.guard.json")).unwrap());
    evidence(
        "guard-publication",
        &json!({"guard_fsynced_before_store":true,"guard_reissue_count":"0","result":"PASS"}),
    );
}
#[cfg(feature = "fault-injection")]
#[test]
fn evidence_errno_faults_preserve_raw_and_close() {
    let mut rows = vec![];
    // Darwin errno values: actual injections only, no disk exhaustion.
    for (errno, label) in [(5, "EIO"), (28, "ENOSPC"), (69, "EDQUOT")] {
        for point in ["evidence_write", "evidence_complete"] {
            let (h, i, v, e) = new(0);
            let mut next = v.clone();
            next["height"] = json!("101");
            finish(&mut next);
            let at = point.to_owned();
            e.set_fault_hook(Some(Arc::new(move |p| {
                if p == at {
                    Err(std::io::Error::from_raw_os_error(errno).into())
                } else {
                    Ok(())
                }
            })))
            .unwrap();
            assert!(
                e.execute(
                    Command::Snapshot(canonical(&next).unwrap()),
                    &[],
                    &observation(&next),
                    NOW
                )
                .is_err()
            );
            assert_eq!(e.reader().get().unwrap().commit.command_seq, 0);
            assert_eq!(e.reader().get().unwrap().gate, "RECOVERY_REQUIRED");
            drop(e);
            assert!(Engine::open(&h, Validated::new(i).unwrap()).is_err());
            rows.push(json!({"point":point,"injected_errno":label,"host_enospc_tested":false,"result":"PASS"}));
        }
    }
    evidence("evidence-faults", &json!(rows));
}
#[test]
fn offline_binary_create_open_and_profile_failure() {
    let (i, v) = initial(0);
    let h = home(0);
    let parent = h.parent().unwrap();
    let input = parent.join("inputs.json");
    let profile = parent.join("profile.json");
    let bootstrap = parent.join("bootstrap.json");
    fs::write(&input, serde_json::to_vec(&bundle(&i)).unwrap()).unwrap();
    fs::write(&profile, &i.effective_profile).unwrap();
    fs::write(&bootstrap, canonical(&v).unwrap()).unwrap();
    let binary = env!("CARGO_BIN_EXE_nus-s3-local-demo");
    let mut rows = vec![];
    for mode in ["validate", "create", "open"] {
        let mut c = Process::new(binary);
        c.arg(mode)
            .arg("--local-demo-profile")
            .arg(&profile)
            .arg("--acknowledge-unproven-space")
            .arg("--runtime-pin")
            .arg(&i.approved_runtime_sha256)
            .arg("--input-set")
            .arg(&input)
            .arg("--home")
            .arg(&h);
        if mode == "create" {
            c.arg("--bootstrap").arg(&bootstrap);
        }
        let output = c.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        rows.push(json!({"mode":mode,"stdout":String::from_utf8_lossy(&output.stdout),"exit":"0"}));
    }
    fs::write(&profile, b"unknown profile").unwrap();
    let output = Process::new(binary)
        .args(["open", "--local-demo-profile"])
        .arg(&profile)
        .arg("--acknowledge-unproven-space")
        .arg("--runtime-pin")
        .arg(&i.approved_runtime_sha256)
        .arg("--input-set")
        .arg(&input)
        .arg("--home")
        .arg(&h)
        .output()
        .unwrap();
    assert!(!output.status.success());
    rows.push(json!({"mode":"unknown-profile","stderr":String::from_utf8_lossy(&output.stderr),"exit":output.status.code().unwrap().to_string()}));
    evidence("offline-binary", &json!(rows));
}

#[test]
fn ancestor_alias_and_outside_namespace_are_rejected_before_creation() {
    let (i, v) = initial(0);
    let h = home(0);
    let parent = h.parent().unwrap();
    let alias = parent.parent().unwrap().join("alias");
    symlink(parent, &alias).unwrap();
    let attempted = alias.join("fee0");
    assert!(
        Engine::create(
            &attempted,
            Validated::new(i.clone()).unwrap(),
            &canonical(&v).unwrap()
        )
        .is_err()
    );
    assert!(!h.exists());
    let outside = parent.join("outside");
    assert!(
        Engine::create(
            &outside,
            Validated::new(i).unwrap(),
            &canonical(&v).unwrap()
        )
        .is_err()
    );
    assert!(!outside.exists());
    evidence(
        "ancestor-confinement",
        &json!({"outside_created":false,"symlink_ancestor_followed":false,"result":"PASS"}),
    );
}

#[test]
#[cfg(feature = "fault-injection")]
fn trusted_recovery_serializes_with_publication_and_stale_commit() {
    use nus_exchange_contract::s3::dev_local::Error;
    let (_, _, v, e) = new(0);
    let e = Arc::new(e);
    let old = e.reader().get().unwrap().commit.clone();
    let (entered, at_barrier) = mpsc::channel();
    let (resume, wait) = mpsc::channel();
    let wait = std::sync::Mutex::new(wait);
    e.set_fault_hook(Some(Arc::new(move |point| {
        if point == "before_publish" {
            entered.send(()).unwrap();
            wait.lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
        Ok(())
    })))
    .unwrap();
    let worker = e.clone();
    let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 244);
    let writer = std::thread::spawn(move || {
        worker.execute(signed(&raw, &sig, 0), &[], &observation(&v), NOW)
    });
    at_barrier
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(e.reader().get().unwrap().commit, old);
    let (started, reading) = mpsc::channel();
    let reader = e.clone();
    let read = std::thread::spawn(move || {
        started.send(()).unwrap();
        reader.trusted_recovery_history(&old, None, 64)
    });
    reading
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    resume.send(()).unwrap();
    assert!(writer.join().unwrap().unwrap().is_some());
    assert!(matches!(
        read.join().unwrap(),
        Err(Error::Invalid("STALE_COMMIT"))
    ));
    let current = e.reader().get().unwrap();
    let page = e
        .trusted_recovery_history(&current.commit, None, 1)
        .unwrap();
    assert_eq!(page.commit, current.commit);
    assert_eq!(page.commit.command_seq, 1);
    assert_eq!(e.reader().get().unwrap().gate, "OPEN");
    evidence(
        "trusted-publisher-barrier",
        &json!({"result":"PASS","barrier":"before_publish","old_revision":0,"new_revision":1,"old_query":"STALE_COMMIT","new_query":"PASS","effect_callbacks":0}),
    );
}

#[test]
#[cfg(feature = "fault-injection")]
fn trusted_recovery_rejects_poisoned_writer() {
    use nus_exchange_contract::s3::{dev_local::Error, schema};
    let (h, _, v, e) = new(0);
    let e = Arc::new(e);
    let before = e.reader().get().unwrap();
    e.set_fault_hook(Some(Arc::new(|point| {
        assert_ne!(point, "before_wal", "injected writer panic");
        Ok(())
    })))
    .unwrap();
    let writer = e.clone();
    let (raw, sig) = sign_order(&v, 0, "2", 1000, 10000, 245);
    assert!(
        std::thread::spawn(move || writer.execute(
            signed(&raw, &sig, 0),
            &[],
            &observation(&v),
            NOW
        ))
        .join()
        .is_err()
    );
    let wal = fs::read(h.join("journal.dev.wal")).unwrap();
    let marker = fs::read(h.join("commit.dev.json")).unwrap();
    assert!(matches!(
        e.trusted_recovery_history(&before.commit, None, 64),
        Err(Error::Recovery("WRITER_POISONED"))
    ));
    assert!(matches!(
        e.trusted_recovery_attempt(&before.commit, schema::ZERO),
        Err(Error::Recovery("WRITER_POISONED"))
    ));
    assert!(matches!(
        e.trusted_recovery_attempt_at(&before.commit, 0),
        Err(Error::Recovery("WRITER_POISONED"))
    ));
    let mut callbacks = 0;
    assert!(
        e.with_committed_attempt(schema::ZERO, |_, _| callbacks += 1)
            .is_err()
    );
    assert_eq!(callbacks, 0);
    assert_eq!(e.reader().get().unwrap().commit, before.commit);
    assert_eq!(e.reader().get().unwrap().state, before.state);
    assert_eq!(e.reader().get().unwrap().receipts, before.receipts);
    assert_eq!(fs::read(h.join("journal.dev.wal")).unwrap(), wal);
    assert_eq!(fs::read(h.join("commit.dev.json")).unwrap(), marker);
    evidence(
        "trusted-writer-poison",
        &json!({"result":"PASS","read_errors":"WRITER_POISONED","callback_calls":0,"commit_diff":[],"state_diff":[],"receipt_diff":[],"wal_diff":[],"marker_diff":[]}),
    );
}

#[test]
fn trusted_recovery_history_page_ceiling_and_bootstrap_bytes() {
    let (h, input, mut v, e) = new(0);
    let bootstrap = fs::read(h.join("bootstrap.dev.json")).unwrap();
    for height in 101..=165 {
        v["height"] = json!(height.to_string());
        v["block_hash"] = json!(sha256(height.to_string().as_bytes()));
        finish(&mut v);
        e.execute(
            Command::Snapshot(canonical(&v).unwrap()),
            &[],
            &observation(&v),
            NOW,
        )
        .unwrap();
    }
    let view = e.reader().get().unwrap();
    drop(e);
    let mut rows = Vec::new();
    for _ in 0..2 {
        let e = Engine::open(&h, Validated::new(input.clone()).unwrap()).unwrap();
        let first = e.trusted_recovery_history(&view.commit, None, 64).unwrap();
        assert_eq!(first.observations.len(), 64);
        assert_eq!(first.observations[0].raw, bootstrap);
        assert_eq!(first.observations[63].snapshot.height(), 163);
        assert_eq!(first.next_height, Some(164));
        assert_eq!(first.applied.snapshot.height(), 100);
        assert_eq!(first.latest.snapshot.height(), 165);
        let last = e
            .trusted_recovery_history(&view.commit, first.next_height, 64)
            .unwrap();
        assert_eq!(last.observations.len(), 2);
        assert_eq!(last.next_height, None);
        assert_eq!(last.observations[0].snapshot.height(), 164);
        assert_eq!(last.observations[1].snapshot.height(), 165);
        assert_eq!(e.reader().get().unwrap().commit, view.commit);
        assert_eq!(e.reader().get().unwrap().state, view.state);
        assert_eq!(e.reader().get().unwrap().receipts, view.receipts);
        rows.push(json!({"first_page_count":64,"next_height":164,"second_page_count":2,"final_next_height":null,"applied_height":100,"latest_height":165,"state_diff":[],"receipt_diff":[],"commit_diff":[]}));
    }
    evidence(
        "trusted-history-ceiling",
        &json!({"result":"PASS","replays":rows,"bootstrap_sha256":sha256(&bootstrap)}),
    );
}

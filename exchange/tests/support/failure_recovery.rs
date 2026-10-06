//! Real store, synthetic RPC, no services. Original failure bytes must exist
//! before CLOSE/VOID receipt; replaying the selection is not a raw byte source.
use super::*;
use nus_exchange_contract::s3::{
    dev_local::{Command, Engine as DevEngine, Error, RecoveryFailure},
    evidence::{self, RPC, TYPED},
};
use std::{collections::BTreeMap, fs, path::Path};

fn prepared(bps: u32) -> (Candidate, Value) {
    let mut c = order(
        order(setup(bps, false), 0, "2", 2000, 10000, 241),
        1,
        "1",
        1000,
        12000,
        242,
    );
    c = replay_check(
        &c,
        c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
        "SEAL_BATCH",
    );
    let a = attempt(&mut c, "SETTLE", None);
    c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
    (c, a)
}
fn failed(bps: u32) -> (Candidate, Value) {
    let (c, a) = prepared(bps);
    finish_failure(c, a)
}
fn finish_failure(mut c: Candidate, a: Value) -> (Candidate, Value) {
    let v = next_snapshot(&c);
    c = observe(c, v);
    let mut terminal = a;
    terminal["state"] = json!("INCLUDED_FAILURE");
    terminal["confirmed_tx"] = proof(&mut c, &terminal, "1019");
    c = replay_check(
        &c,
        c.resolve_attempt(terminal.clone()).unwrap(),
        "RESOLVE_ATTEMPT",
    );
    (c, terminal)
}

fn files(root: &Path) -> BTreeMap<String, String> {
    fn visit(root: &Path, at: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    sha256(&fs::read(path).unwrap()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
fn returned(r: &RecoveryFailure) -> Value {
    json!({"commit":{"command_seq":r.commit.command_seq,"record_hash":r.commit.record_hash,"end_offset":r.commit.end_offset},
        "reference":r.resolution_evidence_ref,"resolution":r.resolution_evidence,"raw_base64":STANDARD.encode(&r.raw),
        "objects":r.evidence.entries().map(|(r,b)|json!({"reference":r,"raw_base64":STANDARD.encode(b)})).collect::<Vec<_>>()})
}
fn check_reads(c: &Candidate, original: &Value, bps: u32, label: &str) -> Value {
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap();
        let baseline = t.dev.engine.as_ref().unwrap().reader().get().unwrap();
        let disk = files(&t.dev.home);
        let batch = original["batch"]["batch_id"].as_str().unwrap();
        let mut runs = Vec::new();
        let mut restored = Value::Null;
        for replay in 0..=2 {
            if replay > 0 {
                drop(t.dev.engine.take());
                t.dev.engine = Some(DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap());
            }
            let e = t.dev.engine.as_ref().unwrap();
            let r = e.trusted_recovery_failure(&baseline.commit, batch).unwrap().unwrap();
            assert_eq!(r.commit, baseline.commit);
            assert_eq!(r.resolution_evidence, *original);
            assert_eq!(r.raw, canonical(original).unwrap());
            assert_eq!(r.resolution_evidence_ref, evidence::reference(&r.raw, TYPED).unwrap());
            let root = json!({"resolution_evidence_ref":r.resolution_evidence_ref});
            let refs = r.evidence.graph(&root).unwrap();
            assert_eq!(r.evidence.entries().count(), refs.len());
            assert!(refs.len() <= 58);
            for (reference, raw) in r.evidence.entries() {
                evidence::verify(reference, raw).unwrap();
                let path = t.dev.home.join("objects/sha256").join(reference["sha256"].as_str().unwrap());
                assert_eq!(fs::read(&path).unwrap(), raw);
                assert_eq!(fs::read(path.with_extension("ref")).unwrap(), canonical(reference).unwrap());
            }
            assert_eq!(r.evidence.typed(&r.resolution_evidence_ref, "ResolutionEvidence").unwrap(), *original);
            // All original proofs remain independently usable with the same
            // commit's bounded history, without a separate WAL parser.
            let h = e.trusted_recovery_history(&baseline.commit, None, 64).unwrap();
            let history: Vec<_> = h.observations.iter().map(|o| &o.snapshot).collect();
            for a in original["settle_attempts"].as_array().unwrap() {
                let tx = a["tx_hash"].as_str().unwrap();
                let old = e.trusted_recovery_attempt(&baseline.commit, tx).unwrap().unwrap();
                assert_eq!(old.attempt, *a);
                if a["state"] == "INCLUDED_FAILURE" {
                    nus_exchange_contract::s3::proof::confirmed(&a["confirmed_tx"], c.latest().context(), &history, &r.evidence).unwrap();
                } else {
                    let selected = h.observations.iter().find(|o| o.snapshot.id() == a["absence_proof"]["observation_snapshot_id"]).unwrap();
                    nus_exchange_contract::s3::proof::absence(a, &selected.snapshot, &history, &r.evidence).unwrap();
                }
                let mut callbacks = 0;
                assert!(matches!(e.with_committed_attempt(tx, |_, _| callbacks += 1), Err(Error::Invalid("ATTEMPT_TERMINAL"))));
                assert_eq!(callbacks, 0);
            }
            for field in ["seq", "hash", "offset"] {
                let mut stale = baseline.commit.clone();
                match field { "seq" => stale.command_seq += 1, "hash" => stale.record_hash = schema::ZERO.into(), _ => stale.end_offset += 1 }
                assert!(matches!(e.trusted_recovery_failure(&stale, batch), Err(Error::Invalid("STALE_COMMIT"))));
                assert!(matches!(e.trusted_recovery_failure(&stale, schema::ZERO), Err(Error::Invalid("STALE_COMMIT"))));
            }
            for invalid in ["", "../profile.guard.json", &"A".repeat(64)] {
                assert!(matches!(e.trusted_recovery_failure(&baseline.commit, invalid), Err(Error::Invalid(_))));
            }
            for missing in [schema::ZERO, original["failed_tx_hash"].as_str().unwrap()] {
                assert!(e.trusted_recovery_failure(&baseline.commit, missing).unwrap().is_none());
            }
            // Returned Values/Objects are detached, including the explicit raw.
            let mut changed = r.clone();
            changed.resolution_evidence["batch"]["batch_id"] = json!(schema::ZERO);
            changed.resolution_evidence_ref["sha256"] = json!(schema::ZERO);
            changed.raw[0] ^= 1;
            changed.evidence.insert(b"{}", RPC).unwrap();
            assert_eq!(returned(&e.trusted_recovery_failure(&baseline.commit, batch).unwrap().unwrap()), returned(&r));
            let after = e.reader().get().unwrap();
            assert_eq!(after.commit, baseline.commit); assert_eq!(after.state, baseline.state);
            assert_eq!(after.receipts, baseline.receipts); assert_eq!(after.gate, baseline.gate);
            assert_eq!(files(&t.dev.home), disk);
            restored = r.resolution_evidence.clone();
            runs.push(json!({"replay":replay,"returned":returned(&r),"callback_calls":0,"state_diff":[],"receipt_diff":[],"store_diff":[]}));
        }
        dev_fixture::evidence(&format!("failure-{label}-fee{bps}"), &json!({"scope":"SYNTHETIC_RPC_REAL_DEV_STORE_COMPONENT","result":"PASS","runs":runs,"files_sha256":disk,"state":baseline.state,"receipt_ledger":baseline.receipts.values().collect::<Vec<_>>() }));
        dev_fixture::copy_home(&format!("failure-{label}"), &t.dev.home);
        restored
    })
}

#[test]
fn failure_recovery_exact_original_two_replays_and_close_consumer() {
    for bps in [0, 25] {
        let (mut c, _) = failed(bps);
        let original = c.rejection_evidence().unwrap();
        let batch = original["batch"]["batch_id"].as_str().unwrap();
        TRACE.with_borrow(|trace| {
            let t = trace.as_ref().unwrap();
            let e = t.dev.engine.as_ref().unwrap();
            let before = e.reader().get().unwrap();
            let disk = files(&t.dev.home);
            assert!(
                e.trusted_recovery_failure(&before.commit, batch)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(files(&t.dev.home), disk);
            assert_eq!(e.reader().get().unwrap().commit, before.commit);
        });
        let before = c.full_state().unwrap();
        c = replay_check(&c, c.reject_final(original.clone()).unwrap(), "VOID_BATCH");
        for k in [
            "accounts",
            "fills",
            "corrections",
            "resolution_receipts",
            "attempt_refs",
            "chain_snapshot",
        ] {
            assert_eq!(before[k], c.full_state().unwrap()[k]);
        }
        check_reads(&c, &original, bps, "rejected");
        // New observations must not silently reselect/change the old evidence.
        let v = next_snapshot(&c);
        c = observe(c, v);
        assert_ne!(original["observed_snapshot"], *c.latest().value());
        let recovered = check_reads(&c, &original, bps, "later-observation");
        let close = attempt(&mut c, "CLOSE", Some(&recovered));
        c = replay_check(&c, c.prepare_attempt(close.clone()).unwrap(), "ATTEMPT");
        let v = terminal_snapshot(&c, &close, false);
        c = observe(c, v);
        let receipt = receipt(&mut c, &close, Some(&recovered));
        c = replay_check(&c, c.record_receipt(receipt).unwrap(), "VOID_BATCH");
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        assert_eq!(c.batches()[0]["state"], "CORRECTED");
        check_reads(&c, &original, bps, "corrected");
    }
}

#[test]
fn failure_recovery_three_attempts_include_all_absence_raws() {
    for bps in [0, 25] {
        let (mut c, mut a) = prepared(bps);
        for number in 1..=2 {
            let start = c.latest().height() + 1;
            let timeout = start + 7;
            let mut prev = c.latest().value()["block_hash"].clone();
            let mut blocks = Vec::new();
            for _ in 0..9 {
                let v = next_snapshot(&c);
                c = observe(c, v);
                if c.latest().height() <= timeout {
                    let s = c.latest().value();
                    let b = json!({"result":{"block_id":{"hash":s["block_hash"]},"block":{"header":{"chain_id":"nus-s3-dev-1","height":s["height"],"last_block_id":{"hash":prev}},"data":{"txs":null}}}});
                    let r = json!({"result":{"height":s["height"],"txs_results":null}});
                    let height = s["height"].clone();
                    let hash = s["block_hash"].clone();
                    let br = c.provide_evidence(&canonical(&b).unwrap(), RPC).unwrap();
                    let rr = c.provide_evidence(&canonical(&r).unwrap(), RPC).unwrap();
                    blocks.push(json!({"height":height,"block_hash":hash,"raw_block_response_ref":br,"raw_results_response_ref":rr}));
                    prev = hash;
                }
            }
            a["state"] = json!("EXPIRED_ABSENT_PROVEN");
            a["absence_proof"] = json!({"tx_hash":a["tx_hash"],"first_possible_height":start.to_string(),"timeout_height":timeout.to_string(),"observed_height":c.latest().height().to_string(),"account_sequence":"0","last_batch_seq":"0","last_batch_hash":schema::ZERO,"receipt_absent":true,"blocks":blocks,"observation_snapshot_id":c.latest().id()});
            c = replay_check(&c, c.resolve_attempt(a).unwrap(), "RESOLVE_ATTEMPT");
            a = attempt(&mut c, "SETTLE", None);
            a["attempt_no"] = json!((number + 1).to_string());
            c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
        }
        let (mut c, _) = finish_failure(c, a);
        let original = c.rejection_evidence().unwrap();
        c = replay_check(&c, c.reject_final(original.clone()).unwrap(), "VOID_BATCH");
        assert_eq!(original["settle_attempts"].as_array().unwrap().len(), 3);
        check_reads(&c, &original, bps, "three-attempts");
    }
}

#[test]
fn failure_recovery_separates_two_batches_at_one_commit() {
    let (mut c, a) = prepared(0);
    // These independent fills survive the first batch's correction.
    c = order(order(c, 2, "2", 1000, 5000, 243), 3, "1", 1000, 5000, 244);
    let (mut c, _) = finish_failure(c, a);
    let first = c.rejection_evidence().unwrap();
    c = replay_check(&c, c.reject_final(first.clone()).unwrap(), "VOID_BATCH");
    let close = attempt(&mut c, "CLOSE", Some(&first));
    c = replay_check(&c, c.prepare_attempt(close.clone()).unwrap(), "ATTEMPT");
    let v = terminal_snapshot(&c, &close, false);
    c = observe(c, v);
    let receipt = receipt(&mut c, &close, Some(&first));
    c = replay_check(&c, c.record_receipt(receipt).unwrap(), "VOID_BATCH");
    c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
    c = replay_check(
        &c,
        c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
        "SEAL_BATCH",
    );
    let a = attempt(&mut c, "SETTLE", None);
    c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
    let second_id = a["batch"]["batch_id"].as_str().unwrap().to_owned();
    TRACE.with_borrow(|trace| {
        let e = trace.as_ref().unwrap().dev.engine.as_ref().unwrap();
        let v = e.reader().get().unwrap();
        assert!(
            e.trusted_recovery_failure(&v.commit, &second_id)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            e.trusted_recovery_failure(&v.commit, first["batch"]["batch_id"].as_str().unwrap())
                .unwrap()
                .unwrap()
                .resolution_evidence,
            first
        );
    });
    let (mut c, _) = finish_failure(c, a);
    let second = c.rejection_evidence().unwrap();
    c = replay_check(&c, c.reject_final(second.clone()).unwrap(), "VOID_BATCH");
    assert_ne!(first["batch"]["batch_id"], second["batch"]["batch_id"]);
    check_reads(&c, &first, 0, "first-batch");
    check_reads(&c, &second, 0, "second-batch");
}

#[test]
fn failure_recovery_tampering_and_missing_original_close_without_repair() {
    let mut rows = Vec::new();
    for change in [
        "root",
        "descriptor",
        "media",
        "length",
        "missing-root",
        "missing-pair",
        "missing-pair-execute",
        "missing-pair-effect",
        "missing-pair-other-batch",
        "wrong-batch",
        "rpc",
        "tx",
        "hardlink",
        "guard",
        "marker",
    ] {
        let (mut c, terminal) = failed(0);
        let original = c.rejection_evidence().unwrap();
        c = replay_check(&c, c.reject_final(original.clone()).unwrap(), "VOID_BATCH");
        let effect_tx = if change == "missing-pair-effect" {
            let close = attempt(&mut c, "CLOSE", Some(&original));
            c = replay_check(&c, c.prepare_attempt(close.clone()).unwrap(), "ATTEMPT");
            close["tx_hash"].as_str().unwrap().to_owned()
        } else {
            terminal["tx_hash"].as_str().unwrap().to_owned()
        };
        TRACE.with_borrow_mut(|trace| {
            let t = trace.as_mut().unwrap(); let e = t.dev.engine.as_ref().unwrap();
            let v = e.reader().get().unwrap();
            let batch = original["batch"]["batch_id"].as_str().unwrap();
            let r = evidence::reference(&canonical(&original).unwrap(), TYPED).unwrap();
            let p = t.dev.home.join("objects/sha256").join(r["sha256"].as_str().unwrap());
            match change {
                "root" => fs::write(&p, b"{}").unwrap(),
                "descriptor" | "media" | "length" => {
                    let mut bad = r.clone();
                    match change { "descriptor" => bad["sha256"] = json!(schema::ZERO), "media" => bad["media_type"] = json!(RPC), _ => bad["byte_length"] = json!("1") }
                    fs::write(p.with_extension("ref"), canonical(&bad).unwrap()).unwrap();
                }
                "missing-root" => fs::remove_file(&p).unwrap(),
                "missing-pair" | "missing-pair-execute" | "missing-pair-effect" | "missing-pair-other-batch" => { fs::remove_file(&p).unwrap(); fs::remove_file(p.with_extension("ref")).unwrap(); }
                "wrong-batch" => {
                    let mut bad = original.clone(); bad["batch"]["batch_id"] = json!(schema::ZERO);
                    let raw = canonical(&bad).unwrap();
                    fs::write(&p, &raw).unwrap();
                    fs::write(p.with_extension("ref"), canonical(&evidence::reference(&raw, TYPED).unwrap()).unwrap()).unwrap();
                }
                "rpc" | "tx" => {
                    let r = if change == "rpc" { &terminal["confirmed_tx"]["raw_results_response_ref"] } else { &terminal["raw_tx_ref"] };
                    fs::write(t.dev.home.join("objects/sha256").join(r["sha256"].as_str().unwrap()), b"{}").unwrap();
                }
                "hardlink" => fs::hard_link(&p, t.dev.home.parent().unwrap().join("alias")).unwrap(),
                "guard" => fs::write(t.dev.home.join("profile.guard.json"), b"{}").unwrap(),
                "marker" => fs::write(t.dev.home.join("commit.dev.json"), b"{}").unwrap(),
                _ => unreachable!(),
            }
            let tampered = files(&t.dev.home);
            let err = match change {
                "missing-pair-execute" => e.execute(Command::Snapshot(canonical(c.latest().value()).unwrap()), &[], &observation(&c), NOW).unwrap_err(),
                "missing-pair-effect" => e.with_committed_attempt(&effect_tx, |_, _| panic!("missing original authorized CLOSE")).unwrap_err(),
                "missing-pair-other-batch" => e.trusted_recovery_failure(&v.commit, schema::ZERO).unwrap_err(),
                _ => e.trusted_recovery_failure(&v.commit, batch).unwrap_err(),
            };
            assert_eq!(e.reader().get().unwrap().gate, "RECOVERY_REQUIRED");
            assert!(matches!(e.trusted_recovery_failure(&v.commit, batch), Err(Error::Recovery("RECOVERY_REQUIRED"))));
            assert!(e.trusted_recovery_history(&v.commit, None, 64).is_err());
            assert!(e.trusted_recovery_attempt_at(&v.commit, 0).is_err());
            assert!(e.execute(Command::RejectFinal, &[], &observation(&c), NOW).is_err());
            let mut callbacks = 0;
            assert!(e.with_committed_attempt(terminal["tx_hash"].as_str().unwrap(), |_, _| callbacks += 1).is_err());
            assert_eq!(callbacks, 0);
            assert_eq!(e.reader().get().unwrap().commit, v.commit); assert_eq!(e.reader().get().unwrap().state, v.state);
            assert_eq!(e.reader().get().unwrap().receipts, v.receipts); assert_eq!(files(&t.dev.home), tampered);
            drop(t.dev.engine.take());
            for _ in 0..2 {
                assert!(DevEngine::open(&t.dev.home, t.dev.config.clone()).is_err());
                assert_eq!(files(&t.dev.home), tampered);
            }
            rows.push(json!({"change":change,"error":err.to_string(),"recovery":"CLOSED","failed_open_attempts":2,"callbacks":callbacks,"commit_diff":[],"state_diff":[],"receipt_diff":[],"post_tamper_file_diff":[]}));
        });
    }
    dev_fixture::evidence("failure-tampering", &json!({"result":"PASS","rows":rows}));
}

#[test]
#[cfg(feature = "fault-injection")]
fn failure_recovery_transaction_faults_never_ack_missing_original() {
    use std::sync::Arc;
    let mut rows = Vec::new();
    for point in [
        "evidence_write",
        "evidence_complete",
        "before_wal",
        "after_wal_sync",
        "after_marker_sync",
        "after_marker_rename",
        "after_marker_dir_sync",
        "after_commit",
        "before_publish",
        "before_response",
    ] {
        let (c, _) = failed(0);
        let original = c.rejection_evidence().unwrap();
        TRACE.with_borrow_mut(|trace| {
            let t = trace.as_mut().unwrap(); let e = t.dev.engine.as_ref().unwrap();
            let before = e.reader().get().unwrap();
            let r = evidence::reference(&canonical(&original).unwrap(), TYPED).unwrap();
            let path = t.dev.home.join("objects/sha256").join(r["sha256"].as_str().unwrap());
            e.set_fault_hook(Some(Arc::new(move |p| {
                if p == point {
                    // Remove both files after writes to prove the pre-WAL
                    // semantic check requires the original, not just a hash.
                    if p == "evidence_complete" { fs::remove_file(&path).unwrap(); fs::remove_file(path.with_extension("ref")).unwrap(); }
                    else { return Err(Error::Io(std::io::Error::from_raw_os_error(28))); }
                }
                Ok(())
            }))).unwrap();
            let err = e.execute(Command::RejectFinal, &[], &observation(&c), NOW).unwrap_err();
            assert_eq!(e.reader().get().unwrap().gate, "RECOVERY_REQUIRED");
            assert!(e.trusted_recovery_failure(&before.commit, original["batch"]["batch_id"].as_str().unwrap()).is_err());
            let disk = files(&t.dev.home);
            let expected_committed = ["after_commit", "before_publish", "before_response"].contains(&point);
            drop(t.dev.engine.take()); let mut replays = Vec::new();
            for replay in 1..=2 {
                let opened = DevEngine::open(&t.dev.home, t.dev.config.clone());
                assert_eq!(opened.is_ok(), expected_committed, "{point}");
                if let Ok(e) = opened {
                    let v = e.reader().get().unwrap();
                    assert_eq!(v.commit.command_seq, before.commit.command_seq + 1);
                    assert_eq!(e.trusted_recovery_failure(&v.commit, original["batch"]["batch_id"].as_str().unwrap()).unwrap().unwrap().raw, canonical(&original).unwrap());
                }
                assert_eq!(files(&t.dev.home), disk);
                replays.push(json!({"replay":replay,"opened":expected_committed,"store_diff":[]}));
            }
            rows.push(json!({"fault":point,"error":err.to_string(),"success_responses":0,"committed_prefix":expected_committed,"replays":replays}));
        });
    }
    dev_fixture::evidence(
        "failure-transaction-faults",
        &json!({"result":"PASS","scope":"INJECTED_IO_NOT_HOST_ENOSPC","rows":rows}),
    );
}

#[test]
#[cfg(feature = "fault-injection")]
fn failure_recovery_waits_for_publication_then_rejects_old_commit() {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;
    let (c, _) = failed(0);
    let original = c.rejection_evidence().unwrap();
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap(); let e = Arc::new(t.dev.engine.take().unwrap());
        let old = e.reader().get().unwrap().commit.clone();
        let (entered, at_barrier) = mpsc::channel(); let (resume, wait) = mpsc::channel(); let wait = Mutex::new(wait);
        e.set_fault_hook(Some(Arc::new(move |point| {
            if point == "before_publish" { entered.send(()).unwrap(); wait.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap(); }
            Ok(())
        }))).unwrap();
        let obs = observation(&c); let worker = e.clone();
        let writer = std::thread::spawn(move || worker.execute(Command::RejectFinal, &[], &obs, NOW));
        at_barrier.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(e.reader().get().unwrap().commit, old);
        let (started, reading) = mpsc::channel(); let (result_tx, result_rx) = mpsc::channel();
        let reader = e.clone(); let batch = original["batch"]["batch_id"].as_str().unwrap().to_owned();
        let read = std::thread::spawn(move || { started.send(()).unwrap(); result_tx.send(reader.trusted_recovery_failure(&old, &batch)).unwrap(); });
        reading.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(result_rx.recv_timeout(Duration::from_millis(30)), Err(mpsc::RecvTimeoutError::Timeout)));
        resume.send(()).unwrap(); assert!(writer.join().unwrap().unwrap().is_some()); read.join().unwrap();
        assert!(matches!(result_rx.recv().unwrap(), Err(Error::Invalid("STALE_COMMIT"))));
        let v = e.reader().get().unwrap();
        let r = e.trusted_recovery_failure(&v.commit, original["batch"]["batch_id"].as_str().unwrap()).unwrap().unwrap();
        assert_eq!(r.resolution_evidence, original);
        e.set_fault_hook(None).unwrap();
        t.dev.engine = Some(Arc::try_unwrap(e).ok().unwrap());
        dev_fixture::evidence("failure-publisher-barrier", &json!({"result":"PASS","barrier":"before_publish","intermediate_returns":0,"old_query":"STALE_COMMIT","new_query":"PASS","effect_callbacks":0}));
    });
}

#[test]
fn failure_recovery_original_bytes_exist_before_close_or_receipt() {
    let (mut c, _) = failed(0);
    let e = c.rejection_evidence().unwrap();
    c = replay_check(&c, c.reject_final(e.clone()).unwrap(), "VOID_BATCH");
    assert_eq!(c.batches()[0]["state"], "REJECTED_FINAL");
    let raw = canonical(&e).unwrap();
    let r = evidence::reference(&raw, TYPED).unwrap();
    TRACE.with_borrow(|t| {
        let t = t.as_ref().unwrap();
        let p = t.dev.home.join("objects/sha256").join(r["sha256"].as_str().unwrap());
        let present = p.is_file();
        dev_fixture::evidence("failure-original-presence", &json!({"root_present":present,"reference":r,"raw_base64":STANDARD.encode(&raw),"commit":format!("{:?}", t.dev.engine.as_ref().unwrap().reader().get().unwrap().commit)}));
        assert!(present, "RejectFinal has no stored ResolutionEvidence root before CLOSE/VOID receipt");
        assert_eq!(fs::read(&p).unwrap(), raw);
        assert_eq!(fs::read(p.with_extension("ref")).unwrap(), canonical(&r).unwrap());
    });
}

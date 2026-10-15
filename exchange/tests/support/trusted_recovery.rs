//! Real development store; synthetic chain responses, no network or services.
use super::*;
use nus_exchange_contract::s3::{
    dev_local::{Command, Engine as DevEngine, Error, RecoveryHistory},
    evidence::{RPC, TX},
};
use std::{collections::BTreeMap, fs, path::Path};

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
fn prepared(bps: u32) -> (Candidate, Value) {
    let mut c = order(
        order(setup(bps, false), 0, "2", 2000, 10000, 231),
        1,
        "1",
        1000,
        12000,
        232,
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
fn page_json(page: &RecoveryHistory) -> Value {
    json!({"commit":format!("{:?}",page.commit),"first_height":page.first_height,
        "applied":page.applied.snapshot.value(),"latest":page.latest.snapshot.value(),
        "observations":page.observations.iter().map(|o|json!({"snapshot":o.snapshot.value(),"raw_base64":STANDARD.encode(&o.raw)})).collect::<Vec<_>>(),
        "next_height":page.next_height})
}
/// All five statuses use the same trusted read path. No broadcast callback is
/// used to obtain evidence; each read must leave every persisted byte unchanged.
fn check_reads(c: &Candidate, attempt: &Value, bps: u32, label: &str, terminal: bool) {
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap();
        let baseline = t.dev.engine.as_ref().unwrap().reader().get().unwrap();
        let disk = files(&t.dev.home);
        let mut runs = Vec::new();
        for replay in 0..=2 {
            if replay > 0 {
                drop(t.dev.engine.take());
                t.dev.engine = Some(DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap());
            }
            let e = t.dev.engine.as_ref().unwrap();
            let hash = attempt["tx_hash"].as_str().unwrap();
            let recovered = e.trusted_recovery_attempt(&baseline.commit, hash).unwrap().unwrap();
            assert_eq!(recovered.commit, baseline.commit);
            assert_eq!(recovered.attempt, *attempt);
            let discovered = e.trusted_recovery_attempt_at(&baseline.commit, 0).unwrap().unwrap();
            assert_eq!(discovered.attempt, *attempt);
            assert_eq!(discovered.commit, baseline.commit);
            assert_eq!(baseline.state["attempt_refs"][0]["sha256"], sha256(&canonical(&discovered.attempt).unwrap()));
            assert!(e.trusted_recovery_attempt_at(&baseline.commit, 1).unwrap().is_none());
            assert!(e.trusted_recovery_attempt_at(&baseline.commit, usize::MAX).unwrap().is_none());
            let raw = recovered.evidence.resolve(&attempt["raw_tx_ref"], TX).unwrap();
            assert_eq!(raw, c.evidence_bytes(&attempt["raw_tx_ref"], TX).unwrap());
            assert_eq!(sha256(raw), hash);
            // No unrelated latest snapshot/order/other-attempt evidence leaks.
            let refs = recovered.evidence.graph(&attempt).unwrap();
            assert_eq!(recovered.evidence.entries().count(), refs.len() + 1);
            for (r, bytes) in recovered.evidence.entries() {
                nus_exchange_contract::s3::evidence::verify(r, bytes).unwrap();
                if r["media_type"] != nus_exchange_contract::s3::evidence::TYPED {
                    assert_eq!(bytes, c.evidence_bytes(r, r["media_type"].as_str().unwrap()).unwrap());
                }
            }
            let mut next = None;
            let mut snapshots = Vec::new();
            let mut pages = Vec::new();
            loop {
                let page = e.trusted_recovery_history(&baseline.commit, next, 3).unwrap();
                assert_eq!(page.commit, baseline.commit);
                assert_eq!(page.applied.snapshot, *c.snapshot());
                assert_eq!(page.latest.snapshot, *c.latest());
                for item in &page.observations {
                    assert_eq!(c.snapshot().decode_related(&item.raw).unwrap(), item.snapshot);
                    if let Some(prev) = snapshots.last() {
                        assert!(nus_exchange_contract::s3::snapshot::Snapshot::advance(prev, &item.snapshot).unwrap());
                    }
                    snapshots.push(item.snapshot.clone());
                }
                next = page.next_height;
                pages.push(page_json(&page));
                if next.is_none() { break; }
            }
            assert_eq!(snapshots.first().unwrap().height(), 100);
            assert_eq!(snapshots.last().unwrap(), c.latest());
            assert_eq!(snapshots.len() as u64, c.latest().height() - 99);
            for field in ["seq", "hash", "offset"] {
                let mut stale = baseline.commit.clone();
                match field { "seq" => stale.command_seq += 1, "hash" => stale.record_hash = schema::ZERO.into(), _ => stale.end_offset += 1 }
                assert!(matches!(e.trusted_recovery_history(&stale, None, 3), Err(Error::Invalid("STALE_COMMIT"))));
                assert!(matches!(e.trusted_recovery_attempt(&stale, hash), Err(Error::Invalid("STALE_COMMIT"))));
                assert!(matches!(e.trusted_recovery_attempt_at(&stale, 0), Err(Error::Invalid("STALE_COMMIT"))));
            }
            for limit in [0, 65, usize::MAX] {
                assert!(matches!(e.trusted_recovery_history(&baseline.commit, None, limit), Err(Error::Invalid("RECOVERY_PAGE_LIMIT"))));
            }
            for height in [0, 99, c.latest().height()+1, u64::MAX] {
                assert!(matches!(e.trusted_recovery_history(&baseline.commit, Some(height), 1), Err(Error::Invalid("RECOVERY_HEIGHT_RANGE"))));
            }
            assert_eq!(e.trusted_recovery_history(&baseline.commit, None, 64).unwrap().observations.len(), snapshots.len());
            assert!(e.trusted_recovery_attempt(&baseline.commit, "../profile.guard.json").is_err());
            assert!(e.trusted_recovery_attempt(&baseline.commit, schema::ZERO).unwrap().is_none());
            let mut callback_count = 0;
            if terminal {
                assert!(matches!(e.with_committed_attempt(hash, |_, _| callback_count += 1), Err(Error::Invalid("ATTEMPT_TERMINAL"))));
            }
            assert_eq!(callback_count, 0);
            let after = e.reader().get().unwrap();
            assert_eq!(after.commit, baseline.commit); assert_eq!(after.state, baseline.state);
            assert_eq!(after.receipts, baseline.receipts); assert_eq!(after.gate, baseline.gate);
            assert_eq!(files(&t.dev.home), disk);
            runs.push(json!({"replay":replay,"pages":pages,"attempt":recovered.attempt,
                "objects":recovered.evidence.entries().map(|(r,b)|json!({"reference":r,"raw_base64":STANDARD.encode(b)})).collect::<Vec<_>>(),
                "callback_calls":callback_count,"state_diff":[],"receipt_diff":[],"store_diff":[]}));
        }
        dev_fixture::evidence(&format!("trusted-{label}-fee{bps}"), &json!({"scope":"SYNTHETIC_RPC_REAL_DEV_STORE_COMPONENT","result":"PASS","runs":runs,"files_sha256":disk,"state":baseline.state,"receipt_ledger":baseline.receipts.values().collect::<Vec<_>>() }));
        dev_fixture::copy_home(&format!("trusted-{label}"), &t.dev.home);
    });
}

#[test]
fn terminal_raw_and_snapshot_apply_gap_survive_two_replays() {
    for bps in [0, 25] {
        for code in ["0", "1021"] {
            let (mut c, a) = prepared(bps);
            check_reads(&c, &a, bps, &format!("prepared-{code}"), false);
            let mut unknown = a.clone();
            unknown["state"] = json!("SUBMISSION_UNKNOWN");
            unknown["broadcast_count"] = json!("1");
            c = replay_check(
                &c,
                c.resolve_attempt(unknown.clone()).unwrap(),
                "RESOLVE_ATTEMPT",
            );
            check_reads(&c, &unknown, bps, &format!("unknown-{code}"), false);
            let v = if code == "0" {
                terminal_snapshot(&c, &a, true)
            } else {
                next_snapshot(&c)
            };
            c = observe(c, v);
            let mut terminal = unknown;
            terminal["state"] = json!(if code == "0" {
                "INCLUDED_SUCCESS"
            } else {
                "INCLUDED_FAILURE"
            });
            terminal["confirmed_tx"] = proof(&mut c, &a, code);
            c = replay_check(
                &c,
                c.resolve_attempt(terminal.clone()).unwrap(),
                "RESOLVE_ATTEMPT",
            );
            assert_eq!(c.snapshot().height(), 100);
            assert_eq!(c.latest().height(), 101);
            check_reads(&c, &terminal, bps, &format!("terminal-{code}"), true);
            if code == "0" {
                let r = receipt(&mut c, &a, None);
                c = replay_check(&c, c.record_receipt(r).unwrap(), "RESOLVE_ATTEMPT");
                c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
                assert_eq!(c.snapshot().height(), 101);
                assert_eq!(c.mode(), "OPEN");
                check_reads(&c, &terminal, bps, "applied", true);
            }
        }
    }
}

#[test]
fn paged_history_restores_absence_proof_at_same_commit() {
    for bps in [0, 25] {
        let (mut c, a) = prepared(bps);
        let mut blocks = Vec::new();
        let mut prev = c.latest().value()["block_hash"].clone();
        for _ in 0..9 {
            let v = next_snapshot(&c);
            c = observe(c, v);
            if c.latest().height() <= 108 {
                let s = c.latest().value().clone();
                let b = json!({"result":{"block_id":{"hash":s["block_hash"]},"block":{"header":{"chain_id":"nus-s3-dev-1","height":s["height"],"last_block_id":{"hash":prev}},"data":{"txs":null}}}});
                let r = json!({"result":{"height":s["height"],"txs_results":null}});
                let br = c.provide_evidence(&canonical(&b).unwrap(), RPC).unwrap();
                let rr = c.provide_evidence(&canonical(&r).unwrap(), RPC).unwrap();
                blocks.push(json!({"height":s["height"],"block_hash":s["block_hash"],"raw_block_response_ref":br,"raw_results_response_ref":rr}));
                prev = s["block_hash"].clone();
            }
        }
        let mut expired = a.clone();
        expired["state"] = json!("EXPIRED_ABSENT_PROVEN");
        expired["absence_proof"] = json!({"tx_hash":a["tx_hash"],"first_possible_height":"101","timeout_height":"108","observed_height":"109","account_sequence":"0","last_batch_seq":"0","last_batch_hash":schema::ZERO,"receipt_absent":true,"blocks":blocks,"observation_snapshot_id":c.latest().id()});
        c = replay_check(
            &c,
            c.resolve_attempt(expired.clone()).unwrap(),
            "RESOLVE_ATTEMPT",
        );
        check_reads(&c, &expired, bps, "absence", true);
        // Receipt collector can re-run the existing proof using only C reads.
        TRACE.with_borrow(|trace| {
            let e = trace.as_ref().unwrap().dev.engine.as_ref().unwrap();
            let commit = e.reader().get().unwrap().commit.clone();
            let h = e.trusted_recovery_history(&commit, None, 64).unwrap();
            let a = e
                .trusted_recovery_attempt(&commit, a["tx_hash"].as_str().unwrap())
                .unwrap()
                .unwrap();
            let history = h
                .observations
                .iter()
                .map(|s| &s.snapshot)
                .collect::<Vec<_>>();
            nus_exchange_contract::s3::proof::absence(
                &a.attempt,
                &h.latest.snapshot,
                &history,
                &a.evidence,
            )
            .unwrap();
        });
        let mut retry = attempt(&mut c, "SETTLE", None);
        retry["attempt_no"] = json!("2");
        c = replay_check(&c, c.prepare_attempt(retry.clone()).unwrap(), "ATTEMPT");
        TRACE.with_borrow_mut(|trace| {
            let t = trace.as_mut().unwrap();
            let view = t.dev.engine.as_ref().unwrap().reader().get().unwrap();
            let disk = files(&t.dev.home);
            let mut rows = Vec::new();
            for replay in 1..=2 {
                drop(t.dev.engine.take());
                let e = DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap();
                let mut found = Vec::new();
                for (index, expected) in c.attempts().iter().enumerate() {
                    let read = e.trusted_recovery_attempt_at(&view.commit, index).unwrap().unwrap();
                    assert_eq!(read.attempt, *expected);
                    assert_eq!(view.state["attempt_refs"][index]["sha256"], sha256(&canonical(&read.attempt).unwrap()));
                    found.push(read.attempt["state"].clone());
                }
                assert_eq!(found, vec![json!("EXPIRED_ABSENT_PROVEN"), json!("PREPARED")]);
                assert!(e.trusted_recovery_attempt_at(&view.commit, 2).unwrap().is_none());
                assert_eq!(e.reader().get().unwrap().commit, view.commit);
                assert_eq!(e.reader().get().unwrap().state, view.state);
                assert_eq!(e.reader().get().unwrap().receipts, view.receipts);
                assert_eq!(files(&t.dev.home), disk);
                rows.push(json!({"replay":replay,"states_by_index":found,"state_diff":[],"receipt_diff":[],"store_diff":[]}));
                t.dev.engine = Some(e);
            }
            dev_fixture::evidence(&format!("trusted-discovery-{bps}"), &json!({"result":"PASS","rows":rows,"sidecar":false}));
        });
    }
}

#[test]
fn tampered_store_and_terminal_evidence_close_both_recovery_apis() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let mut rows = Vec::new();
    for change in [
        "raw",
        "descriptor",
        "media",
        "length",
        "missing",
        "symlink",
        "hardlink",
        "guard",
        "wal_inode",
        "root_inode",
        "marker",
    ] {
        let (mut c, a) = prepared(0);
        let v = terminal_snapshot(&c, &a, true);
        c = observe(c, v);
        let mut terminal = a.clone();
        terminal["state"] = json!("INCLUDED_SUCCESS");
        terminal["confirmed_tx"] = proof(&mut c, &a, "0");
        c = replay_check(
            &c,
            c.resolve_attempt(terminal.clone()).unwrap(),
            "RESOLVE_ATTEMPT",
        );
        TRACE.with_borrow(|trace| {
            let t = trace.as_ref().unwrap(); let e = t.dev.engine.as_ref().unwrap();
            let view = e.reader().get().unwrap();
            let object = t.dev.home.join("objects/sha256").join(a["raw_tx_ref"]["sha256"].as_str().unwrap());
            let mut original = fs::read(&object).unwrap();
            match change {
                "raw" => { original[0] ^= 1; fs::write(&object, original).unwrap(); },
                "descriptor" | "media" | "length" => {
                    let p = object.with_extension("ref"); let mut r = a["raw_tx_ref"].clone();
                    match change { "descriptor" => r["sha256"] = json!(schema::ZERO), "media" => r["media_type"] = json!(RPC), _ => r["byte_length"] = json!("1") }
                    fs::write(p, canonical(&r).unwrap()).unwrap();
                },
                "missing" => fs::remove_file(&object).unwrap(),
                "symlink" => { fs::remove_file(&object).unwrap(); symlink(t.dev.home.join("profile.guard.json"), &object).unwrap(); },
                "hardlink" => fs::hard_link(&object, t.dev.home.parent().unwrap().join("alias")).unwrap(),
                "guard" => fs::write(t.dev.home.join("profile.guard.json"), b"{}").unwrap(),
                "wal_inode" => {
                    let wal = t.dev.home.join("journal.dev.wal"); let copy = fs::read(&wal).unwrap();
                    fs::remove_file(&wal).unwrap(); fs::write(&wal, copy).unwrap(); fs::set_permissions(&wal, fs::Permissions::from_mode(0o600)).unwrap();
                },
                "root_inode" => { fs::rename(&t.dev.home, t.dev.home.with_extension("old")).unwrap(); fs::create_dir(&t.dev.home).unwrap(); },
                "marker" => fs::write(t.dev.home.join("commit.dev.json"), b"{}").unwrap(),
                _ => unreachable!(),
            }
            let changed = files(&t.dev.home);
            // Alternate which API discovers the error; the other must stay closed.
            if ["raw", "media", "missing", "hardlink", "wal_inode"].contains(&change) {
                assert!(e.trusted_recovery_attempt(&view.commit, a["tx_hash"].as_str().unwrap()).is_err());
            } else { assert!(e.trusted_recovery_history(&view.commit, None, 64).is_err()); }
            assert_eq!(e.reader().get().unwrap().gate, "RECOVERY_REQUIRED");
            assert!(matches!(e.trusted_recovery_history(&view.commit, None, 64), Err(Error::Recovery("RECOVERY_REQUIRED"))));
            assert!(matches!(e.trusted_recovery_attempt(&view.commit, a["tx_hash"].as_str().unwrap()), Err(Error::Recovery("RECOVERY_REQUIRED"))));
            assert!(matches!(e.trusted_recovery_attempt_at(&view.commit, 0), Err(Error::Recovery("RECOVERY_REQUIRED"))));
            let mut callbacks = 0;
            assert!(e.with_committed_attempt(a["tx_hash"].as_str().unwrap(), |_, _| callbacks += 1).is_err());
            assert_eq!(callbacks, 0);
            assert!(e.execute(Command::Apply, &[], &observation(&c), NOW).is_err());
            assert_eq!(e.reader().get().unwrap().commit, view.commit); assert_eq!(e.reader().get().unwrap().state, view.state);
            assert_eq!(e.reader().get().unwrap().receipts, view.receipts); assert_eq!(files(&t.dev.home), changed);
            rows.push(json!({"change":change,"recovery":"CLOSED","callbacks":callbacks,"commit_diff":[],"state_diff":[],"receipt_diff":[],"post_tamper_file_diff":[]}));
        });
    }
    dev_fixture::evidence("trusted-tampering", &json!({"result":"PASS","rows":rows}));
}

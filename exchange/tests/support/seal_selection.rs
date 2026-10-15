//! Trusted selection on real dev stores; chain observations remain synthetic.
use super::*;
use nus_exchange_contract::s3::dev_local::{
    ApplyReadiness as Apply, Engine as DevEngine, Error, SealPurpose as Purpose,
    SealReadiness as Seal, SealWait as Wait,
};
use std::{collections::BTreeMap, fs, path::Path};
fn files(root: &Path) -> BTreeMap<String, String> {
    fn visit(root: &Path, at: &Path, out: &mut BTreeMap<String, String>) {
        for item in fs::read_dir(at).unwrap() {
            let p = item.unwrap().path();
            if p.is_dir() {
                visit(root, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    sha256(&fs::read(&p).unwrap()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
fn queued(bps: u32) -> Candidate {
    order(
        order(setup(bps, true), 0, "2", 2000, 10000, 141),
        1,
        "1",
        1000,
        12000,
        142,
    )
}
fn check(c: &Candidate, bps: u32, label: &str, seal: Seal, apply: Apply) {
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap();
        let before = t.dev.engine.as_ref().unwrap().reader().get().unwrap();
        let disk = files(&t.dev.home);
        let mut runs = vec![];
        for replay in 0..=2 {
            if replay > 0 {
                drop(t.dev.engine.take());
                t.dev.engine = Some(DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap());
            }
            let e = t.dev.engine.as_ref().unwrap();
            let r = e.trusted_reconcile_readiness(&before.commit, &observation(c), NOW).unwrap();
            assert_eq!(r.commit, before.commit); assert_eq!(r.seal, seal, "{label}"); assert_eq!(r.apply, apply, "{label}");
            assert_eq!(r.applied_height, c.snapshot().height()); assert_eq!(r.latest_height, c.latest().height());
            // Owned return values cannot change engine state/next selection.
            let mut changed = r.clone(); changed.commit.command_seq = u64::MAX;
            changed.seal = Seal::Ready(Purpose::ResolveFailure); changed.apply = Apply::Ready;
            assert_eq!(e.trusted_reconcile_readiness(&before.commit, &observation(c), NOW).unwrap(), r);
            let after = e.reader().get().unwrap();
            assert_eq!(after.commit, before.commit); assert_eq!(after.state, before.state);
            assert_eq!(after.gate, before.gate); assert_eq!(after.receipts, before.receipts);
            assert_eq!(files(&t.dev.home), disk);
            e.reconcile_receipt_ledger(&before.receipts.values().cloned().collect::<Vec<_>>()).unwrap();
            runs.push(json!({"replay":replay,"seal":format!("{:?}",r.seal),"apply":format!("{:?}",r.apply),
                "command_seq":r.commit.command_seq,"record_hash":r.commit.record_hash,"end_offset":r.commit.end_offset,
                "applied_height":r.applied_height,"latest_height":r.latest_height,
                "state_diff":[],"store_diff":[],"receipt_diff":[],"callback_calls":0}));
        }
        dev_fixture::evidence(&format!("selection-{label}-fee{bps}"), &json!({
            "scope":"SYNTHETIC_CHAIN_REAL_DEV_STORE_COMPONENT", "result":"PASS",
            "expected_seal":format!("{seal:?}"),"expected_apply":format!("{apply:?}"),
            "latest_observation":c.latest().value(),"state":before.state,"runs":runs,
            "files_sha256":disk,"receipt_ledger":before.receipts.values().collect::<Vec<_>>() }));
        dev_fixture::copy_home(&format!("selection-{label}"), &t.dev.home);
    });
}
#[test]
fn fifo_empty_pending_observation_and_normal_seal() {
    for bps in [0, 25] {
        let mut c = setup(bps, true);
        check(
            &c,
            bps,
            "empty",
            Seal::Waiting(Wait::EmptyQueue),
            Apply::NoPendingObservation,
        );
        let v = next_snapshot(&c);
        c = observe(c, v);
        check(
            &c,
            bps,
            "empty-pending",
            Seal::Waiting(Wait::EmptyQueue),
            Apply::Ready,
        );
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        c = order(order(c, 0, "2", 2000, 10000, 143), 1, "1", 1000, 12000, 144);
        c = order(c, 2, "1", 1000, 12000, 145);
        check(
            &c,
            bps,
            "fifo",
            Seal::Ready(Purpose::Normal),
            Apply::NoPendingObservation,
        );
        let expected = c.full_state().unwrap()["fills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["fill_id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(expected.len(), 2);
        let v = next_snapshot(&c);
        c = observe(c, v);
        check(
            &c,
            bps,
            "fifo-pending",
            Seal::Waiting(Wait::ApplyPending),
            Apply::Ready,
        );
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        c = replay_check(
            &c,
            c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
            "SEAL_BATCH",
        );
        assert_eq!(c.batches()[0]["batch"]["fill_ids"], json!(expected));
        check(
            &c,
            bps,
            "fifo-sealed",
            Seal::ActiveBatch {
                batch: c.batches()[0]["batch"].clone(),
                state: "SEALED".into(),
            },
            Apply::NoPendingObservation,
        );
    }
}
#[test]
fn operator_owner_epoch_and_revoke_select_resolution_without_release() {
    for bps in [0, 25] {
        for cause in ["operator", "owner", "revoke"] {
            let mut c = queued(bps);
            let mut v = next_snapshot(&c);
            if cause == "operator" {
                v["operator_epoch"] = json!("2");
            } else {
                let hash = c
                    .orders()
                    .values()
                    .find(|o| o.live.owner == owner(0))
                    .unwrap()
                    .live
                    .hash
                    .clone();
                if cause == "owner" {
                    for a in v["accounts"].as_array_mut().unwrap() {
                        if a["owner"] == owner(0) {
                            a["epoch"] = json!("1");
                        }
                    }
                }
                v["owner_events"] = json!([{"kind":if cause=="owner"{"BUMP_EPOCH"}else{"REVOKE_ORDER"},"owner":owner(0),
                "before_epoch":"0","after_epoch":if cause=="owner"{"1"}else{"0"},"order_hash":if cause=="revoke"{json!(hash)}else{Value::Null},
                "denom":"NONE","amount_atoms":"0","request_id":"22".repeat(32),"tx_hash":"33".repeat(32),"tx_index":"1"}]);
            }
            c = observe(c, v);
            assert_eq!(c.apply().unwrap_err(), "UNSETTLED_HOLD");
            check(
                &c,
                bps,
                &format!("{cause}-pending"),
                Seal::Ready(Purpose::ResolveFailure),
                Apply::Held {
                    reason: "UNSETTLED_HOLD",
                },
            );
            let before = c.full_state().unwrap();
            c = replay_check(
                &c,
                c.seal_batch("RESOLVE_FAILURE", &observation(&c), NOW)
                    .unwrap(),
                "SEAL_BATCH",
            );
            let after = c.full_state().unwrap();
            for k in [
                "accounts",
                "orders",
                "attempt_refs",
                "corrections",
                "chain_snapshot",
            ] {
                assert_eq!(after[k], before[k], "{k}");
            }
            assert!(c.attempts().is_empty());
            check(
                &c,
                bps,
                &format!("{cause}-sealed"),
                Seal::ActiveBatch {
                    batch: c.batches()[0]["batch"].clone(),
                    state: "SEALED".into(),
                },
                Apply::Held {
                    reason: "UNSETTLED_HOLD",
                },
            );
        }
    }
}
fn expiring_order(c: Candidate, i: usize, side: &str, id: u8) -> Candidate {
    let (raw, _) = sign_order(&c, i, side, 1000, 10000, id);
    let mut v = Codec::default().decode("OrderV1", &raw).unwrap();
    v["expiry_height"] = json!("120");
    let raw = Codec::default().encode("OrderV1", &v).unwrap();
    let sig = sign(i, &codec::frame("NUS/ORDER/V1", &raw));
    let (next, r, _) = c
        .submit("ORDER", &raw, &sig, &owner(i), &observation(&c), NOW)
        .unwrap();
    assert_eq!(r.code, "OK");
    replay_check(&c, next, "ORDER")
}
#[test]
fn expiry_margin_twelve_eleven_and_actual_expiry() {
    for bps in [0, 25] {
        let mut c = expiring_order(expiring_order(setup(bps, true), 0, "2", 146), 1, "1", 147);
        while c.latest().height() < 108 {
            let v = next_snapshot(&c);
            c = observe(c, v);
        }
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        check(
            &c,
            bps,
            "margin12",
            Seal::Ready(Purpose::Normal),
            Apply::NoPendingObservation,
        );
        let v = next_snapshot(&c);
        c = observe(c, v);
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        check(
            &c,
            bps,
            "margin11",
            Seal::Waiting(Wait::ExpiryMargin),
            Apply::NoPendingObservation,
        );
        assert_eq!(
            c.seal_batch("NORMAL", &observation(&c), NOW).unwrap_err(),
            "EXPIRY_MARGIN"
        );
        assert_eq!(
            c.seal_batch("RESOLVE_FAILURE", &observation(&c), NOW)
                .unwrap_err(),
            "SEAL_PURPOSE"
        );
        while c.latest().height() < 120 {
            let v = next_snapshot(&c);
            c = observe(c, v);
        }
        check(
            &c,
            bps,
            "expired-pending",
            Seal::Ready(Purpose::ResolveFailure),
            Apply::Ready,
        );
        // Follow the documented priority: Apply first, then query the new commit.
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        check(
            &c,
            bps,
            "expired-applied",
            Seal::Ready(Purpose::ResolveFailure),
            Apply::NoPendingObservation,
        );
        c = replay_check(
            &c,
            c.seal_batch("RESOLVE_FAILURE", &observation(&c), NOW)
                .unwrap(),
            "SEAL_BATCH",
        );
        assert!(c.attempts().is_empty());
    }
}
#[test]
fn active_unknown_terminal_and_receipt_apply_boundaries() {
    for bps in [0, 25] {
        let mut c = queued(bps);
        c = replay_check(
            &c,
            c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
            "SEAL_BATCH",
        );
        let a = attempt(&mut c, "SETTLE", None);
        c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
        let mut unknown = a.clone();
        unknown["state"] = json!("SUBMISSION_UNKNOWN");
        unknown["broadcast_count"] = json!("1");
        c = replay_check(
            &c,
            c.resolve_attempt(unknown.clone()).unwrap(),
            "RESOLVE_ATTEMPT",
        );
        check(
            &c,
            bps,
            "active-unknown",
            Seal::ActiveBatch {
                batch: a["batch"].clone(),
                state: "SUBMISSION_UNKNOWN".into(),
            },
            Apply::NoPendingObservation,
        );
        let v = terminal_snapshot(&c, &a, true);
        c = observe(c, v);
        check(
            &c,
            bps,
            "active-await-receipt",
            Seal::ActiveBatch {
                batch: a["batch"].clone(),
                state: "SUBMISSION_UNKNOWN".into(),
            },
            Apply::Held {
                reason: "UNSETTLED_HOLD",
            },
        );
        unknown["state"] = json!("INCLUDED_SUCCESS");
        unknown["confirmed_tx"] = proof(&mut c, &a, "0");
        c = replay_check(&c, c.resolve_attempt(unknown).unwrap(), "RESOLVE_ATTEMPT");
        let r = receipt(&mut c, &a, None);
        c = replay_check(&c, c.record_receipt(r).unwrap(), "RESOLVE_ATTEMPT");
        check(
            &c,
            bps,
            "receipt-apply",
            Seal::Waiting(Wait::EmptyQueue),
            Apply::Ready,
        );
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        check(
            &c,
            bps,
            "committed",
            Seal::Waiting(Wait::EmptyQueue),
            Apply::NoPendingObservation,
        );
        TRACE.with_borrow(|t| {
            let e = t.as_ref().unwrap().dev.engine.as_ref().unwrap();
            let mut n = 0;
            assert!(matches!(
                e.with_committed_attempt(a["tx_hash"].as_str().unwrap(), |_, _| n += 1),
                Err(Error::Invalid("ATTEMPT_TERMINAL"))
            ));
            assert_eq!(n, 0);
        });
    }
}
#[test]
fn stale_commit_freshness_and_storage_errors_never_authorize() {
    for case in ["stale", "guard", "wal", "latest-object", "latest-pair"] {
        let mut c = queued(0);
        let v = next_snapshot(&c);
        c = observe(c, v);
        TRACE.with_borrow(|t| {
            let t=t.as_ref().unwrap();let e=t.dev.engine.as_ref().unwrap();let before=e.reader().get().unwrap();
            let mut errors=vec![];
            if case=="stale" {
                for field in ["seq","hash","offset"] { let mut stale=before.commit.clone();match field {"seq"=>stale.command_seq+=1,"hash"=>stale.record_hash=schema::ZERO.into(),_=>stale.end_offset+=1};
                    let err=e.trusted_reconcile_readiness(&stale,&observation(&c),NOW).unwrap_err();assert!(matches!(err,Error::Invalid("STALE_COMMIT")));errors.push(err.to_string()); }
                for field in ["snapshot","height","time","latency","catching_up"] { let mut o=observation(&c);let mut now=NOW;
                    match field {"snapshot"=>o.snapshot_id=schema::ZERO.into(),"height"=>o.cursor_height-=1,"time"=>now+=5001,"latency"=>o.query_latency_ms=2001,_=>o.catching_up=true};
                    let err=e.trusted_reconcile_readiness(&before.commit,&o,now).unwrap_err();assert!(matches!(err,Error::Invalid(_)));errors.push(err.to_string()); }
                assert!(e.trusted_reconcile_readiness(&before.commit,&observation(&c),NOW).is_ok());
            } else {
                let p=if case=="guard" {t.dev.home.join("profile.guard.json")} else if case=="wal" {t.dev.home.join("journal.dev.wal")}
                    else {t.dev.home.join("objects/sha256").join(before.state["latest_observation_ref"]["sha256"].as_str().unwrap())};
                if case=="latest-pair" { fs::remove_file(&p).unwrap(); fs::remove_file(p.with_extension("ref")).unwrap(); }
                else { let mut raw=fs::read(&p).unwrap();raw[0]^=1;fs::write(&p,raw).unwrap(); }
                let disk=files(&t.dev.home);
                errors.push(e.trusted_reconcile_readiness(&before.commit,&observation(&c),NOW).unwrap_err().to_string());
                assert_eq!(e.reader().get().unwrap().gate,"RECOVERY_REQUIRED");
                assert!(matches!(e.execute(nus_exchange_contract::s3::dev_local::Command::Apply,&[],&observation(&c),NOW),Err(Error::Recovery("RECOVERY_REQUIRED"))));
                let mut n=0;assert!(e.with_committed_attempt(schema::ZERO,|_,_|n+=1).is_err());assert_eq!(n,0);assert_eq!(files(&t.dev.home),disk);
            }
            let after=e.reader().get().unwrap();assert_eq!(after.commit,before.commit);assert_eq!(after.state,before.state);assert_eq!(after.receipts,before.receipts);
            dev_fixture::evidence(&format!("selection-rejection-{case}"),&json!({"result":"PASS","errors":errors,"commit_diff":[],"state_diff":[],"receipt_diff":[],"callback_calls":0}));
        });
        if case != "stale" {
            TRACE.with_borrow_mut(|trace| {
                let t = trace.as_mut().unwrap();
                let disk = files(&t.dev.home);
                drop(t.dev.engine.take());
                for _ in 0..2 {
                    assert!(DevEngine::open(&t.dev.home, t.dev.config.clone()).is_err());
                }
                assert_eq!(files(&t.dev.home), disk);
            });
        }
    }
}

#[test]
fn inconsistent_chain_cursor_is_error_without_purpose_retry() {
    let c = queued(0);
    let mut v = next_snapshot(&c);
    v["last_batch_seq"] = json!("1");
    v["last_batch_hash"] = json!("12".repeat(32));
    v["terminal_batch_seqs"] = json!(["1"]);
    let c = observe(c, v);
    assert_eq!(c.apply().unwrap_err(), "RECEIPT_INCONSISTENCY");
    TRACE.with_borrow(|t| {let t=t.as_ref().unwrap();let e=t.dev.engine.as_ref().unwrap();let before=e.reader().get().unwrap();let disk=files(&t.dev.home);
        assert!(matches!(e.trusted_reconcile_readiness(&before.commit,&observation(&c),NOW),Err(Error::Invalid("RECEIPT_INCONSISTENCY"))));
        assert_eq!(e.reader().get().unwrap().commit,before.commit);assert_eq!(e.reader().get().unwrap().state,before.state);assert_eq!(files(&t.dev.home),disk);
        dev_fixture::evidence("selection-unexpected-apply-error",&json!({"result":"PASS","error":"RECEIPT_INCONSISTENCY","purpose_retries":0,"state_diff":[],"store_diff":[],"receipt_diff":[]}));
    });
}

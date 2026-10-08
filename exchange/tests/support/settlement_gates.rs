//! Independent reviewer probes, real Engine/store, synthetic keys and evidence.
use super::*;
use nus_exchange_contract::s3::{
    dev_local::Engine as DevEngine,
    settlement_local::{LoopbackRpc, Worker},
};
use std::sync::Arc;

#[test]
fn cto_broadcast_rejects_invalid_freshness_before_intent_and_callback() {
    let mut violations = Vec::new();
    for bps in [0, 25] {
        for mode in [
            "stale_query",
            "stale_block",
            "catching_up",
            "snapshot_conflict",
            "cursor_gap",
        ] {
            let (c, a) = failure_recovery::prepared(bps);
            TRACE.with_borrow_mut(|trace| {
                let t = trace.as_mut().unwrap();
                let e = Arc::new(t.dev.engine.take().unwrap());
                let worker = Worker::new(e.clone());
                let before = e.reader().get().unwrap();
                let files_before = failure_recovery::files(&t.dev.home);
                let mut obs = observation(&c);
                let mut now = NOW;
                match mode {
                    "stale_query" => obs.received_at = NOW - 6001,
                    "stale_block" => { now = NOW + 6001; obs.received_at = now; },
                    "catching_up" => obs.catching_up = true,
                    "snapshot_conflict" => obs.snapshot_id = schema::ZERO.into(),
                    "cursor_gap" => obs.cursor_height += 1,
                    _ => unreachable!(),
                }
                let freshness_error = c.latest().freshness(&obs, now).unwrap_err();
                let mut callbacks = 0;
                let result = worker.test_broadcast(a["tx_hash"].as_str().unwrap(), &obs, now, |stored, raw| {
                    callbacks += 1;
                    assert_eq!(sha256(raw), a["tx_hash"]);
                    assert_eq!(stored["state"], "SUBMISSION_UNKNOWN");
                });
                let after = e.reader().get().unwrap();
                let files_after = failure_recovery::files(&t.dev.home);
                let record = json!({
                    "scope":"NO_SOCKET_COMPONENT", "fee_bps":bps.to_string(), "case":mode,
                    "freshness_error":freshness_error, "worker_return":format!("{result:?}"),
                    "callbacks":callbacks.to_string(), "expected_callbacks":"0",
                    "before_seq":before.commit.command_seq.to_string(), "after_seq":after.commit.command_seq.to_string(),
                    "files_unchanged":files_before == files_after,
                    "accounts_unchanged":before.state["accounts"] == after.state["accounts"],
                    "before_state":before.state, "after_state":after.state,
                    "before_receipts":before.receipts.values().collect::<Vec<_>>(),
                    "after_receipts":after.receipts.values().collect::<Vec<_>>(),
                    "files_before":files_before,"files_after":files_after,
                });
                let accepted = result.is_ok() || callbacks != 0 || before.commit != after.commit;
                if accepted { violations.push(format!("fee{bps}/{mode}")); }
                drop(worker); drop(e);
                for _ in 0..2 {
                    let reopened = DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap();
                    let replayed = reopened.reader().get().unwrap();
                    assert_eq!(replayed.commit, after.commit);
                    assert_eq!(replayed.state, after.state);
                    assert_eq!(replayed.receipts, after.receipts);
                }
                dev_fixture::copy_home(&format!("cto-broadcast-fee{bps}-{mode}"), &t.dev.home);
                dev_fixture::evidence(&format!("cto-broadcast-fee{bps}-{mode}"), &record);
            });
        }
    }
    dev_fixture::evidence(
        "cto-broadcast-summary",
        &json!({"violations":violations,"cases":"10","expected":"all invalid observations rejected before intent commit and callback"}),
    );
    assert!(
        violations.is_empty(),
        "invalid observations reached broadcast: {violations:?}"
    );
}

use nus_exchange_contract::s3::dev_local::{Command, Error, View};
fn same(
    e: &DevEngine,
    before: &View,
    disk: &std::collections::BTreeMap<String, String>,
    home: &std::path::Path,
) {
    let after = e.reader().get().unwrap();
    assert_eq!(after.commit, before.commit);
    assert_eq!(after.state, before.state);
    assert_eq!(after.receipts, before.receipts);
    assert_eq!(&failure_recovery::files(home), disk);
}
fn replays(
    config: &nus_exchange_contract::s3::dev_local::Validated,
    home: &std::path::Path,
    before: &View,
) {
    for _ in 0..2 {
        let e = DevEngine::open(home, config.clone()).unwrap();
        let after = e.reader().get().unwrap();
        assert_eq!(after.commit, before.commit);
        assert_eq!(after.state, before.state);
        assert_eq!(after.receipts, before.receipts);
    }
}
#[test]
fn broadcast_backoff_ages_fresh_observation_without_new_intent() {
    for bps in [0, 25] {
        let (c, a) = failure_recovery::prepared(bps);
        TRACE.with_borrow_mut(|trace| {
            let t=trace.as_mut().unwrap();
            let e=Arc::new(t.dev.engine.take().unwrap());
            let w=Worker::new(e.clone());
            let hash=a["tx_hash"].as_str().unwrap();
            w.test_broadcast(hash,&observation(&c),NOW,|_,_|()).unwrap();
            let before=e.reader().get().unwrap();
            let disk=failure_recovery::files(&t.dev.home);
            let mut calls=0;
            // At entry both ages are 4,500 ms. Durable count1 forces 1,000ms
            // backoff, so using the old `now` would incorrectly broadcast.
            let result=w.test_broadcast(hash,&observation(&c),NOW+4500,|_,_|calls+=1);
            assert!(matches!(result,Err(Error::Invalid("STALE"))),"{result:?}");
            assert_eq!(calls,0); same(&e,&before,&disk,&t.dev.home);
            drop(w);drop(e);replays(&t.dev.config,&t.dev.home,&before);
            dev_fixture::copy_home(&format!("gate-backoff-fee{bps}"),&t.dev.home);
            dev_fixture::evidence(&format!("gate-backoff-fee{bps}"),&json!({"result":"PASS","error":"STALE","callbacks":calls,"intent_delta":0,"commit":before.commit.command_seq,"files_unchanged":true,"replays":2}));
        });
    }
}
#[test]
fn broadcast_snapshot_race_rejects_pinned_intent_and_allows_reconciliation() {
    for bps in [0, 25] {
        let (c, a) = failure_recovery::prepared(bps);
        let raw = canonical(&finish_snapshot(next_snapshot(&c))).unwrap();
        TRACE.with_borrow_mut(|trace| {
            let t=trace.as_mut().unwrap();let e=Arc::new(t.dev.engine.take().unwrap());
            let w=Worker::new(e.clone());let before=e.reader().get().unwrap();
            let mut barrier_calls=0;let mut callbacks=0;let mut snapshot_view=None;let mut disk=None;
            let result=w.test_broadcast_with_clock(a["tx_hash"].as_str().unwrap(),&observation(&c),||{
                barrier_calls+=1;
                assert_eq!(barrier_calls,1,"stale commit must stop before writer clock");
                // Pinned old commit -> scheduling/backoff window -> new Snapshot.
                // Reconcile deliberately remains usable with catching_up=true.
                let mut catching=observation(&c);catching.catching_up=true;
                w.reconcile(Command::Snapshot(raw.clone()),&[],&catching,NOW).unwrap();
                snapshot_view=Some(e.reader().get().unwrap());disk=Some(failure_recovery::files(&t.dev.home));
                Ok(NOW)
            },|_,_|callbacks+=1);
            assert!(matches!(result,Err(Error::Invalid("STALE_COMMIT"))),"{result:?}");
            assert_eq!(callbacks,0);let after=snapshot_view.unwrap();
            assert_eq!(after.commit.command_seq,before.commit.command_seq+1);
            same(&e,&after,&disk.unwrap(),&t.dev.home);
            assert_eq!(after.state["attempts"],before.state["attempts"]);
            assert_eq!(after.state["accounts"],before.state["accounts"]);
            drop(w);drop(e);replays(&t.dev.config,&t.dev.home,&after);
            dev_fixture::copy_home(&format!("gate-race-fee{bps}"),&t.dev.home);
            dev_fixture::evidence(&format!("gate-race-fee{bps}"),&json!({"result":"PASS","error":"STALE_COMMIT","callbacks":callbacks,"snapshot_commits":1,"broadcast_commits":0,"replays":2}));
        });
    }
}
#[test]
fn broadcast_post_marker_expiry_retains_unknown_without_callback() {
    for bps in [0, 25] {
        let (c, a) = failure_recovery::prepared(bps);
        TRACE.with_borrow_mut(|trace| {
            let t=trace.as_mut().unwrap();let e=Arc::new(t.dev.engine.take().unwrap());let w=Worker::new(e.clone());
            let before=e.reader().get().unwrap();let mut samples=0;let mut callbacks=0;
            let result=w.test_broadcast_with_clock(a["tx_hash"].as_str().unwrap(),&observation(&c),||{
                samples+=1;Ok(if samples<3 {NOW} else {NOW+6001})
            },|_,_|callbacks+=1);
            assert!(matches!(result,Err(Error::Invalid("STALE"))),"{result:?}");
            assert_eq!(samples,3);assert_eq!(callbacks,0);
            let after=e.reader().get().unwrap();assert_eq!(after.commit.command_seq,before.commit.command_seq+1);
            assert_eq!(after.state["accounts"],before.state["accounts"]);
            let a=e.committed_attempt(a["tx_hash"].as_str().unwrap()).unwrap().unwrap();
            assert_eq!(a["state"],"SUBMISSION_UNKNOWN");assert_eq!(a["broadcast_count"],"1");
            drop(w);drop(e);replays(&t.dev.config,&t.dev.home,&after);
            dev_fixture::copy_home(&format!("gate-marker-expiry-fee{bps}"),&t.dev.home);
            dev_fixture::evidence(&format!("gate-marker-expiry-fee{bps}"),&json!({"result":"PASS","error":"STALE","callbacks":callbacks,"intent_delta":1,"account_diff":[],"attempt":a,"replays":2}));
        });
    }
}
#[test]
fn public_broadcast_samples_wall_time_after_old_adapter_clock() {
    for bps in [0, 25] {
        let (c, a) = failure_recovery::prepared(bps);
        // Fixture time is historic. The production entry must reject it before
        // the RPC object can issue a socket operation, even if passed old now.
        assert!(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                > u128::from(NOW + 5000)
        );
        TRACE.with_borrow_mut(|trace| {
            let t=trace.as_mut().unwrap();let e=Arc::new(t.dev.engine.take().unwrap());let w=Worker::new(e.clone());
            let before=e.reader().get().unwrap();let disk=failure_recovery::files(&t.dev.home);
            let rpc=LoopbackRpc::new("127.0.0.1:1".parse().unwrap(),std::time::Duration::from_millis(10)).unwrap();
            let result=w.broadcast(a["tx_hash"].as_str().unwrap(),&rpc,&observation(&c),NOW);
            assert!(matches!(result,Err(Error::Invalid("STALE"))),"{result:?}");
            same(&e,&before,&disk,&t.dev.home);
            dev_fixture::evidence(&format!("gate-real-clock-fee{bps}"),&json!({"result":"PASS","error":"STALE","intent_delta":0,"socket_path_entered":false}));
        });
    }
}
#[test]
fn broadcast_accepts_exact_freshness_boundary_and_rechecks_before_effect() {
    for bps in [0, 25] {
        let (c, a) = failure_recovery::prepared(bps);
        TRACE.with_borrow_mut(|trace| {
            let t=trace.as_mut().unwrap();let e=Arc::new(t.dev.engine.take().unwrap());let w=Worker::new(e.clone());
            let before=e.reader().get().unwrap();let mut calls=0;let mut samples=0;
            w.test_broadcast_with_clock(a["tx_hash"].as_str().unwrap(),&observation(&c),||{samples+=1;Ok(NOW+5000)},|stored,raw|{
                calls+=1;assert_eq!(sha256(raw),a["tx_hash"]);assert_eq!(stored["broadcast_count"],"1");
                assert_eq!(e.reader().get().unwrap().commit.command_seq,before.commit.command_seq+1);
            }).unwrap();
            assert_eq!(samples,3);assert_eq!(calls,1);
            dev_fixture::copy_home(&format!("gate-boundary-fee{bps}"),&t.dev.home);
            dev_fixture::evidence(&format!("gate-boundary-fee{bps}"),&json!({"result":"PASS","age_ms":5000,"clock_samples":samples,"callbacks":calls,"intent_delta":1}));
        });
    }
}

#[test]
fn broadcast_intent_to_callback_excludes_concurrent_snapshot_writer() {
    for bps in [0, 25] {
        let (c, a) = failure_recovery::prepared(bps);
        let raw = canonical(&finish_snapshot(next_snapshot(&c))).unwrap();
        TRACE.with_borrow_mut(|trace| {
            let t=trace.as_mut().unwrap();let e=Arc::new(t.dev.engine.take().unwrap());let w=Worker::new(e.clone());
            let before=e.reader().get().unwrap();let hash=a["tx_hash"].as_str().unwrap();
            let (begin,started)=std::sync::mpsc::channel();
            let (ready,waiting)=std::sync::mpsc::channel();
            let (done,finished)=std::sync::mpsc::channel();
            let waiting=std::sync::Mutex::new(waiting);
            let fired=std::sync::atomic::AtomicBool::new(false);
            e.set_fault_hook(Some(Arc::new(move |point| {
                if point=="before_response" && !fired.swap(true,std::sync::atomic::Ordering::SeqCst) {
                    begin.send(()).unwrap(); waiting.lock().unwrap().recv().unwrap();
                }
                Ok(())
            }))).unwrap();
            let writer=e.clone();let obs=observation(&c);
            let thread=std::thread::spawn(move || {
                started.recv().unwrap();ready.send(()).unwrap();
                let r=writer.execute(Command::Snapshot(raw),&[],&obs,NOW);
                done.send(r).unwrap();
            });
            let mut calls=0;
            w.test_broadcast_with_clock(hash,&observation(&c),||Ok(NOW),|_,_| {
                calls+=1;
                assert!(finished.recv_timeout(std::time::Duration::from_millis(40)).is_err());
                let at_effect=e.reader().get().unwrap();
                assert_eq!(at_effect.commit.command_seq,before.commit.command_seq+1);
                assert_eq!(at_effect.state["chain_snapshot"],before.state["chain_snapshot"]);
            }).unwrap();
            assert_eq!(calls,1);assert!(finished.recv().unwrap().is_ok());thread.join().unwrap();
            let after=e.reader().get().unwrap();assert_eq!(after.commit.command_seq,before.commit.command_seq+2);
            assert_eq!(after.state["accounts"],before.state["accounts"]);
            drop(w);drop(e);replays(&t.dev.config,&t.dev.home,&after);
            dev_fixture::copy_home(&format!("gate-atomic-fee{bps}"),&t.dev.home);
            dev_fixture::evidence(&format!("gate-atomic-fee{bps}"),&json!({"result":"PASS","callbacks":calls,"intent_before_callback":true,"snapshot_writer_waited_until_effect":true,"sequence_delta":2,"replays":2}));
        });
    }
}

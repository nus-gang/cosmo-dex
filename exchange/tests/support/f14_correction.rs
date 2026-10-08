//! F14 exact boundary: real Engine + dev store, synthetic signed chain data.
use super::*;
use nus_exchange_contract::s3::dev_local::{
    Command, CorrectionBoundary, CorrectionPhase as Phase, Engine as DevEngine, Error, View,
};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
pub(super) fn ready(bps: u32) -> (Candidate, Vec<String>) {
    let mut c = order(
        order(setup(bps, true), 0, "2", 2000, 10000, 11),
        1,
        "1",
        1000,
        12000,
        12,
    );
    c = replay_check(
        &c,
        c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
        "SEAL_BATCH",
    );
    let a = attempt(&mut c, "SETTLE", None);
    c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
    c = order(c, 2, "1", 1000, 12000, 13); // A's same order -> F2
    c = order(c, 3, "2", 1000, 10000, 14);
    c = order(c, 2, "1", 1000, 12000, 15); // C QUOTE -> F3
    c = order(c, 1, "2", 1000, 20000, 16);
    c = order(c, 3, "1", 1000, 20000, 17); // B BASE/D QUOTE -> independent F4
    let before = c.full_state().unwrap();
    let ids: Vec<_> = before["fills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["fill_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids.len(), 4);
    let mut v = next_snapshot(&c);
    for row in v["accounts"].as_array_mut().unwrap() {
        if row["owner"] == owner(0) {
            row["epoch"] = json!("1");
        }
    }
    v["owner_events"] = json!([{"kind":"BUMP_EPOCH","owner":owner(0),"before_epoch":"0","after_epoch":"1","order_hash":null,"denom":"NONE","amount_atoms":"0","request_id":"22".repeat(32),"tx_hash":"33".repeat(32),"tx_index":"1"}]);
    c = observe(c, v);
    assert_eq!(c.apply().unwrap_err(), "UNSETTLED_HOLD");
    let mut failed = a.clone();
    failed["state"] = json!("INCLUDED_FAILURE");
    failed["confirmed_tx"] = proof(&mut c, &a, "1019");
    c = replay_check(
        &c,
        c.resolve_attempt(failed.clone()).unwrap(),
        "RESOLVE_ATTEMPT",
    );
    let e = json!({"context":c.latest().context(),"batch":a["batch"],"observed_snapshot":c.latest().value(),"settle_attempts":[failed],"batch_lookup":{"context":c.latest().context(),"observed_height":c.latest().height().to_string(),"snapshot_id":c.latest().id(),"requested_seq":a["batch"]["batch_seq"],"last_seq":"0","last_hash":schema::ZERO,"status":"NOT_FOUND_AT_HEIGHT","receipt":null},"failed_tx_hash":a["tx_hash"],"rejection_code":"EPOCH_MISMATCH"});
    let mut incomplete = e.clone();
    incomplete["settle_attempts"] = json!([]);
    assert!(c.reject_final(incomplete).is_err());
    c = replay_check(&c, c.reject_final(e.clone()).unwrap(), "VOID_BATCH");
    let close = attempt(&mut c, "CLOSE", Some(&e));
    c = replay_check(&c, c.prepare_attempt(close.clone()).unwrap(), "ATTEMPT");
    let v = terminal_snapshot(&c, &close, false);
    c = observe(c, v);
    let r = receipt(&mut c, &close, Some(&e));
    c = replay_check(&c, c.record_receipt(r.clone()).unwrap(), "VOID_BATCH");
    (c, ids)
}
fn take() -> DevTrace {
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap();
        DevTrace {
            engine: t.dev.engine.take(),
            config: t.dev.config.clone(),
            home: t.dev.home.clone(),
        }
    })
}
fn files(root: &Path) -> BTreeMap<String, String> {
    fn visit(root: &Path, at: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(at).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                visit(root, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    sha256(&fs::read(p).unwrap()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
fn same(a: &View, b: &View) {
    assert_eq!(a.commit, b.commit);
    assert_eq!(a.state, b.state);
    assert_eq!(a.receipts, b.receipts);
}
fn view(v: &View) -> Value {
    json!({"commit":{"command_seq":v.commit.command_seq,"record_hash":v.commit.record_hash,"end_offset":v.commit.end_offset},"state":v.state,"receipt_ledger":v.receipts.values().collect::<Vec<_>>(),"gate":v.gate})
}
fn event(v: &CorrectionBoundary) -> Value {
    json!({"phase":format!("{:?}",v.phase),"roots":v.root_fill_ids,"corrected":v.corrected_fill_ids,"surviving":v.surviving_fill_ids})
}
fn closure(v: &CorrectionBoundary, ids: &[String]) {
    let mut roots = ids[..2].to_vec();
    roots.sort();
    assert_eq!(v.root_fill_ids, roots);
    assert_eq!(v.corrected_fill_ids, ids[..3]);
    assert_eq!(v.surviving_fill_ids, ids[3..]);
}
fn two_opens(d: &mut DevTrace, expected: &View, succeeds: bool) -> Vec<Value> {
    drop(d.engine.take());
    let disk = files(&d.home);
    let mut rows = vec![];
    for run in 1..=2 {
        match DevEngine::open(&d.home, d.config.clone()) {
            Ok(e) => {
                assert!(succeeds);
                let v = e.reader().get().unwrap();
                same(expected, &v);
                e.reconcile_receipt_ledger(
                    &expected.receipts.values().cloned().collect::<Vec<_>>(),
                )
                .unwrap();
                rows.push(json!({"run":run,"view":view(&v),"result":"PASS"}));
            }
            Err(e) => {
                assert!(!succeeds);
                assert!(matches!(e, Error::Recovery(_)));
                rows.push(
                    json!({"run":run,"error":e.to_string(),"result":"EXPECTED_RECOVERY_REQUIRED"}),
                );
            }
        }
        assert_eq!(disk, files(&d.home));
    }
    rows
}
fn gate(e: &DevEngine, c: &Candidate) {
    let before = e.reader().get().unwrap();
    for command in [
        Command::Apply,
        Command::RejectFinal,
        Command::Seal("NORMAL".into()),
    ] {
        assert!(matches!(
            e.execute(command, &[], &observation(c), NOW),
            Err(Error::Recovery("RECOVERY_REQUIRED"))
        ));
    }
    let (raw, sig) = sign_order(c, 0, "2", 2000, 10000, 11);
    assert!(
        e.query_signed("ORDER", &raw, &sig, &owner(0), &observation(c), NOW)
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        e.execute(
            dev_fixture::signed(&raw, &sig, 0),
            &[],
            &observation(c),
            NOW
        ),
        Err(Error::Recovery("RECOVERY_REQUIRED"))
    ));
    assert!(e.trusted_recovery_history(&before.commit, None, 1).is_err());
    let mut callbacks = 0;
    assert!(
        e.with_committed_attempt(
            c.attempts().last().unwrap()["tx_hash"].as_str().unwrap(),
            |_, _| callbacks += 1
        )
        .is_err()
    );
    assert_eq!(callbacks, 0);
    same(&before, &e.reader().get().unwrap());
}
#[test]
fn f14_exact_phases_barriers_and_missed_selector_match_normal() {
    for bps in [0, 25] {
        let mut baseline = None;
        for mode in ["normal", "observe", "missed"] {
            let (c, ids) = ready(bps);
            let expected = c.apply().unwrap().full_state().unwrap();
            let mut d = take();
            let e = Arc::new(d.engine.take().unwrap());
            let before = e.reader().get().unwrap();
            let wal = fs::read(d.home.join("journal.dev.wal")).unwrap();
            let visits = Arc::new(Mutex::new(vec![]));
            let (tx, rx) = mpsc::channel();
            let (resume, resumed) = mpsc::channel();
            let resumed = Mutex::new(resumed);
            if mode != "normal" {
                let visits = visits.clone();
                let ids = ids.clone();
                let reader = e.reader();
                let before = before.clone();
                e.set_correction_hook(Some(Arc::new(move |v| {
                    closure(v, &ids);
                    same(&before, &reader.get().unwrap());
                    let mut copy = v.clone();
                    copy.corrected_fill_ids.clear();
                    assert_ne!(copy, *v);
                    visits.lock().unwrap().push(event(v));
                    if mode == "observe" {
                        tx.send(v.phase).unwrap();
                        resumed
                            .lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(15))
                            .unwrap();
                    }
                    if mode == "missed" && visits.lock().unwrap().len() == 3 {
                        return Err(Error::Invalid("THIRD_VISIT"));
                    }
                    Ok(())
                })))
                .unwrap();
            }
            // Readiness and direct calculations do not consume execute phases.
            e.trusted_reconcile_readiness(&before.commit, &observation(&c), NOW)
                .unwrap();
            c.apply().unwrap();
            assert!(visits.lock().unwrap().is_empty());
            let worker = e.clone();
            let o = observation(&c);
            let join = std::thread::spawn(move || worker.execute(Command::Apply, &[], &o, NOW));
            if mode == "observe" {
                for phase in [Phase::Prepare, Phase::SemanticReplay] {
                    assert_eq!(rx.recv_timeout(Duration::from_secs(15)).unwrap(), phase);
                    for _ in 0..128 {
                        same(&before, &e.reader().get().unwrap());
                    }
                    assert_eq!(wal, fs::read(d.home.join("journal.dev.wal")).unwrap());
                    assert_eq!(
                        phase == Phase::SemanticReplay,
                        d.home.join("transaction.dev").exists()
                    );
                    resume.send(()).unwrap();
                }
            }
            let result = join.join().unwrap().unwrap().unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state, expected);
            assert_eq!(
                after.state["corrections"][0]["corrected_fill_ids"],
                json!(ids[..3])
            );
            assert_eq!(
                after.state["corrections"][0]["surviving_fill_ids"],
                json!(ids[3..])
            );
            assert_eq!(result["durable_ack"], false);
            assert_eq!(result["command_result"]["kind"], "CORRECTION");
            assert!(
                fs::read(d.home.join("journal.dev.wal"))
                    .unwrap()
                    .starts_with(&wal)
            );
            let current = view(&after);
            if let Some(ref b) = baseline {
                assert_eq!(b, &current);
            } else {
                baseline = Some(current.clone());
            }
            assert_eq!(
                visits.lock().unwrap().len(),
                if mode == "normal" { 0 } else { 2 }
            );
            assert!(
                e.execute(Command::Apply, &[], &observation(&c), NOW)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                visits.lock().unwrap().len(),
                if mode == "normal" { 0 } else { 2 }
            );
            drop(e);
            let replays = two_opens(&mut d, &after, true);
            dev_fixture::evidence(
                &format!("f14-{mode}-fee{bps}"),
                &json!({"before":view(&before),"after":current,"visits":*visits.lock().unwrap(),"expected_ids":ids,"receipt":result,"replays":replays,"files":files(&d.home),"result":"PASS","scope":"SYNTHETIC_COMPONENT"}),
            );
            dev_fixture::copy_home(&format!("f14-{mode}-fee{bps}"), &d.home);
        }
    }
}
#[test]
fn f14_returned_errors_close_lane_and_preserve_originals() {
    for bps in [0, 25] {
        for phase in [Phase::Prepare, Phase::SemanticReplay] {
            for kind in ["invalid", "io"] {
                let (c, ids) = ready(bps);
                let mut d = take();
                let e = d.engine.as_ref().unwrap();
                let before = e.reader().get().unwrap();
                let original = files(&d.home);
                let visits = Arc::new(Mutex::new(vec![]));
                let trace = visits.clone();
                e.set_correction_hook(Some(Arc::new(move |v| {
                    closure(v, &ids);
                    trace.lock().unwrap().push(event(v));
                    if v.phase == phase {
                        if kind == "io" {
                            return Err(Error::Io(std::io::Error::from_raw_os_error(28)));
                        }
                        return Err(Error::Invalid("F14_INJECTED"));
                    }
                    Ok(())
                })))
                .unwrap();
                let err = e
                    .execute(Command::Apply, &[], &observation(&c), NOW)
                    .unwrap_err();
                if kind == "io" {
                    assert!(matches!(&err,Error::Io(x) if x.raw_os_error()==Some(28)));
                } else {
                    assert!(matches!(err, Error::Recovery("F14_INJECTED")));
                }
                let after = e.reader().get().unwrap();
                same(&before, &after);
                assert_eq!(after.gate, "RECOVERY_REQUIRED");
                gate(e, &c);
                let disk = files(&d.home);
                for (path, hash) in &original {
                    assert_eq!(disk.get(path), Some(hash));
                }
                assert_eq!(
                    d.home.join("transaction.dev").exists(),
                    phase == Phase::SemanticReplay
                );
                if phase == Phase::Prepare {
                    assert_eq!(disk, original);
                }
                let count = if phase == Phase::Prepare { 1 } else { 2 };
                assert_eq!(visits.lock().unwrap().len(), count);
                // Scope is gone: a rejected execution cannot leak callbacks into callers.
                c.apply().unwrap();
                assert_eq!(visits.lock().unwrap().len(), count);
                let replays = two_opens(&mut d, &before, phase == Phase::Prepare);
                dev_fixture::evidence(
                    &format!("f14-error-{phase:?}-{kind}-fee{bps}"),
                    &json!({"before":view(&before),"after":view(&after),"visits":*visits.lock().unwrap(),"error":err.to_string(),"files_before":original,"files_after":disk,"replays":replays,"callbacks":0,"result":"PASS"}),
                );
                dev_fixture::copy_home(&format!("f14-error-{phase:?}-{kind}-fee{bps}"), &d.home);
            }
        }
    }
}
#[test]
#[ignore]
fn f14_crash_child() {
    let home = std::path::PathBuf::from(std::env::var_os("NUS70_F14_HOME").unwrap());
    let bps: u32 = std::env::var("NUS70_F14_BPS").unwrap().parse().unwrap();
    let phase = std::env::var("NUS70_F14_PHASE").unwrap();
    let bootstrap: Value =
        serde_json::from_slice(&fs::read(home.join("bootstrap.dev.json")).unwrap()).unwrap();
    let config = nus_exchange_contract::s3::dev_local::Validated::new(dev_fixture::inputs(
        bps,
        bootstrap["accounts"].as_array().unwrap(),
    ))
    .unwrap();
    let e = DevEngine::open(&home, config).unwrap();
    let before = e.reader().get().unwrap();
    let history = e
        .trusted_recovery_history(&before.commit, None, 64)
        .unwrap();
    let o = dev_fixture::observation(history.latest.snapshot.value());
    let stage = std::path::PathBuf::from(std::env::var_os("NUS70_F14_STAGE").unwrap());
    let visits = Arc::new(Mutex::new(vec![]));
    e.set_correction_hook(Some(Arc::new(move |v| {
        visits.lock().unwrap().push(event(v));
        if format!("{:?}", v.phase) == phase {
            let raw =
                serde_json::to_vec(&json!({"visits":*visits.lock().unwrap(),"view":view(&before)}))
                    .unwrap();
            let tmp = stage.with_extension("tmp");
            fs::write(&tmp, raw).unwrap();
            fs::File::open(&tmp).unwrap().sync_all().unwrap();
            fs::rename(tmp, &stage).unwrap();
            loop {
                std::thread::park();
            }
        }
        Ok(())
    })))
    .unwrap();
    e.execute(Command::Apply, &[], &o, NOW).unwrap();
    panic!("F14 missed");
}
#[test]
fn f14_process_kill_and_two_same_home_replays() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command as Process, Stdio};
    for bps in [0, 25] {
        for phase in [Phase::Prepare, Phase::SemanticReplay] {
            let (c, ids) = ready(bps);
            let mut d = take();
            let before = d.engine.as_ref().unwrap().reader().get().unwrap();
            let original = files(&d.home);
            drop(d.engine.take());
            let stage = d
                .home
                .parent()
                .unwrap()
                .join(format!("f14-{phase:?}-stage.json"));
            let log = stage.with_extension("log");
            let output = fs::File::create(&log).unwrap();
            let mut child = Process::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "f14_correction::f14_crash_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("NUS70_F14_HOME", &d.home)
                .env("NUS70_F14_BPS", bps.to_string())
                .env("NUS70_F14_PHASE", format!("{phase:?}"))
                .env("NUS70_F14_STAGE", &stage)
                .stdout(Stdio::from(output.try_clone().unwrap()))
                .stderr(Stdio::from(output))
                .spawn()
                .unwrap();
            let started = std::time::Instant::now();
            while !stage.exists() {
                if let Some(code) = child.try_wait().unwrap() {
                    panic!("child exited {code}: {}", fs::read_to_string(&log).unwrap());
                }
                if started.elapsed() > Duration::from_secs(20) {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("child boundary timeout");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(matches!(
                DevEngine::open(&d.home, d.config.clone()),
                Err(Error::WriterAlreadyRunning)
            ));
            child.kill().unwrap();
            let status = child.wait().unwrap();
            assert_eq!(status.signal(), Some(9));
            let hit: Value = serde_json::from_slice(&fs::read(&stage).unwrap()).unwrap();
            assert_eq!(
                hit["visits"].as_array().unwrap().len(),
                if phase == Phase::Prepare { 1 } else { 2 }
            );
            for v in hit["visits"].as_array().unwrap() {
                assert_eq!(v["corrected"], json!(ids[..3]));
                assert_eq!(v["surviving"], json!(ids[3..]));
            }
            assert_eq!(hit["view"], view(&before));
            let disk = files(&d.home);
            for (path, hash) in &original {
                assert_eq!(disk.get(path), Some(hash));
            }
            let replays = two_opens(&mut d, &before, phase == Phase::Prepare);
            let mut completed = Value::Null;
            if phase == Phase::Prepare {
                let e = DevEngine::open(&d.home, d.config.clone()).unwrap();
                let visits = Arc::new(Mutex::new(vec![]));
                let trace = visits.clone();
                e.set_correction_hook(Some(Arc::new(move |v| {
                    trace.lock().unwrap().push(event(v));
                    Ok(())
                })))
                .unwrap();
                e.execute(Command::Apply, &[], &observation(&c), NOW)
                    .unwrap()
                    .unwrap();
                let after = e.reader().get().unwrap();
                assert_eq!(after.state, c.apply().unwrap().full_state().unwrap());
                assert_eq!(
                    hit["visits"][0]["corrected"],
                    visits.lock().unwrap()[0]["corrected"]
                );
                completed = json!({"view":view(&after),"visits":*visits.lock().unwrap()});
                drop(e);
                two_opens(&mut d, &after, true);
            }
            dev_fixture::evidence(
                &format!("f14-kill-{phase:?}-fee{bps}"),
                &json!({"before":view(&before),"hit":hit,"signal":9,"files_before":original,"files_after_kill":disk,"replays":replays,"completed":completed,"child_log":fs::read_to_string(log).unwrap(),"result":"PASS","scope":"PROCESS_SIGKILL_NOT_POWER_LOSS"}),
            );
            dev_fixture::copy_home(&format!("f14-kill-{phase:?}-fee{bps}"), &d.home);
        }
    }
}

#[test]
fn f14_non_void_and_noop_never_visit() {
    for bps in [0, 25] {
        let mut c = setup(bps, true);
        TRACE.with_borrow(|t| {
            t.as_ref()
                .unwrap()
                .dev
                .engine
                .as_ref()
                .unwrap()
                .set_correction_hook(Some(Arc::new(|_| panic!("non-VOID F14"))))
                .unwrap()
        });
        let v = next_snapshot(&c);
        c = observe(c, v);
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        let d = take();
        let e = d.engine.unwrap();
        assert!(
            e.execute(Command::Apply, &[], &observation(&c), NOW)
                .unwrap()
                .is_none()
        );
        dev_fixture::evidence(
            &format!("f14-nonvoid-fee{bps}"),
            &json!({"view":view(&e.reader().get().unwrap()),"visits":0,"result":"PASS"}),
        );
    }
}
#[test]
fn f14_panic_restores_scope_and_poison_blocks_writer() {
    let (c, _) = ready(0);
    let mut d = take();
    let e = Arc::new(d.engine.take().unwrap());
    let before = e.reader().get().unwrap();
    let original = files(&d.home);
    e.set_correction_hook(Some(Arc::new(|_| panic!("F14 deliberate panic"))))
        .unwrap();
    let worker = e.clone();
    let candidate = c.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            worker.execute(Command::Apply, &[], &observation(&candidate), NOW)
        }));
        assert!(result.is_err());
        // Same thread after unwind: the TLS callback must be gone.
        candidate.apply().unwrap();
    })
    .join()
    .unwrap();
    assert!(matches!(
        e.execute(Command::Apply, &[], &observation(&c), NOW),
        Err(Error::Recovery("WRITER_POISONED"))
    ));
    assert!(
        e.with_committed_attempt("unused", |_, _| panic!("effect after poison"))
            .is_err()
    );
    same(&before, &e.reader().get().unwrap());
    assert_eq!(original, files(&d.home));
    drop(e);
    let replays = two_opens(&mut d, &before, true);
    dev_fixture::evidence(
        "f14-panic",
        &json!({"before":view(&before),"replays":replays,"files":original,"callbacks":0,"result":"PASS"}),
    );
    dev_fixture::copy_home("f14-panic", &d.home);
}

// Retained CTO regressions from the approved F14 review.

#[test]
fn cto_f14_instance_scope_and_clear_do_not_leak() {
    for bps in [0, 25] {
        let (first, ids) = ready(bps);
        let mut a = take();
        let (second, _) = ready(bps);
        let mut b = take();
        let e1 = a.engine.as_ref().unwrap();
        let e2 = b.engine.as_ref().unwrap();
        let visits = Arc::new(Mutex::new(vec![]));
        let recorded = visits.clone();
        e1.set_correction_hook(Some(Arc::new(move |v| {
            closure(v, &ids);
            recorded.lock().unwrap().push(event(v));
            Ok(())
        })))
        .unwrap();
        e2.set_correction_hook(Some(Arc::new(|_| panic!("cleared instance hook fired"))))
            .unwrap();
        e2.set_correction_hook(None).unwrap();
        // Same thread, other Engine executes first; registered e1 hook is dormant.
        e2.execute(Command::Apply, &[], &observation(&second), NOW)
            .unwrap()
            .unwrap();
        assert!(visits.lock().unwrap().is_empty());
        first.apply().unwrap();
        assert!(visits.lock().unwrap().is_empty());
        let b_after = e2.reader().get().unwrap();
        assert_eq!(b_after.state, second.apply().unwrap().full_state().unwrap());
        e1.execute(Command::Apply, &[], &observation(&first), NOW)
            .unwrap()
            .unwrap();
        assert_eq!(
            visits
                .lock()
                .unwrap()
                .iter()
                .map(|v| v["phase"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["Prepare", "SemanticReplay"]
        );
        first.apply().unwrap();
        assert_eq!(visits.lock().unwrap().len(), 2);
        let a_after = e1.reader().get().unwrap();
        same(&a_after, &b_after);
        assert!(
            e1.execute(Command::Apply, &[], &observation(&first), NOW)
                .unwrap()
                .is_none()
        );
        assert!(
            e2.execute(Command::Apply, &[], &observation(&second), NOW)
                .unwrap()
                .is_none()
        );
        assert_eq!(visits.lock().unwrap().len(), 2);
        let ar = two_opens(&mut a, &a_after, true);
        let br = two_opens(&mut b, &b_after, true);
        dev_fixture::evidence(
            &format!("cto-f14-instance-fee{bps}"),
            &json!({"result":"PASS","same_thread":true,"visits":*visits.lock().unwrap(),"first":view(&a_after),"second":view(&b_after),"first_replays":ar,"second_replays":br}),
        );
        dev_fixture::copy_home(&format!("cto-f14-instance-a-fee{bps}"), &a.home);
        dev_fixture::copy_home(&format!("cto-f14-instance-b-fee{bps}"), &b.home);
    }
}

#[test]
fn cto_f14_semantic_panic_resets_scope_and_keeps_transaction() {
    for bps in [0, 25] {
        let (c, ids) = ready(bps);
        let mut d = take();
        let e = d.engine.as_ref().unwrap();
        let before = e.reader().get().unwrap();
        let original = files(&d.home);
        let visits = Arc::new(Mutex::new(vec![]));
        let recorded = visits.clone();
        e.set_correction_hook(Some(Arc::new(move |v| {
            closure(v, &ids);
            recorded.lock().unwrap().push(event(v));
            if v.phase == Phase::SemanticReplay {
                panic!("CTO semantic phase deliberate panic");
            }
            Ok(())
        })))
        .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            e.execute(Command::Apply, &[], &observation(&c), NOW)
        }));
        assert!(result.is_err());
        c.apply().unwrap(); // Same thread: both correction and IO scopes must unwind.
        assert_eq!(visits.lock().unwrap().len(), 2);
        same(&before, &e.reader().get().unwrap());
        assert!(matches!(
            e.execute(Command::Apply, &[], &observation(&c), NOW),
            Err(Error::Recovery("WRITER_POISONED"))
        ));
        assert!(matches!(
            e.set_correction_hook(None),
            Err(Error::Recovery("WRITER_POISONED"))
        ));
        let mut callbacks = 0;
        assert!(
            e.with_committed_attempt("unused", |_, _| callbacks += 1)
                .is_err()
        );
        assert_eq!(callbacks, 0);
        let disk = files(&d.home);
        assert!(disk.contains_key("transaction.dev"));
        for (path, hash) in &original {
            assert_eq!(disk.get(path), Some(hash));
        }
        let replays = two_opens(&mut d, &before, false);
        dev_fixture::evidence(
            &format!("cto-f14-semantic-panic-fee{bps}"),
            &json!({"result":"PASS","before":view(&before),"visits":*visits.lock().unwrap(),"files_before":original,"files_after":disk,"callbacks":callbacks,"replays":replays}),
        );
        dev_fixture::copy_home(&format!("cto-f14-semantic-panic-fee{bps}"), &d.home);
    }
}

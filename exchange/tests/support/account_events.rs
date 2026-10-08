use super::*;
use nus_exchange_contract::s3::dev_local::{Command, Engine as DevEngine};
#[test]
fn account_correction_and_commit_events_preserve_historical_projection() {
    for bps in [0, 25] {
        for correction in [true, false] {
            let mut c = if correction {
                f14_correction::ready(bps).0
            } else {
                let mut c = order(
                    order(setup(bps, true), 0, "2", 2000, 10000, 61),
                    1,
                    "1",
                    1000,
                    12000,
                    62,
                );
                c = replay_check(
                    &c,
                    c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
                    "SEAL_BATCH",
                );
                let a = attempt(&mut c, "SETTLE", None);
                c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
                let v = terminal_snapshot(&c, &a, true);
                c = observe(c, v);
                let r = receipt(&mut c, &a, None);
                c = replay_check(&c, c.record_receipt(r).unwrap(), "RESOLVE_ATTEMPT");
                c
            };
            let historical = TRACE.with_borrow(|t| {
                t.as_ref()
                    .unwrap()
                    .dev
                    .engine
                    .as_ref()
                    .unwrap()
                    .account_receipt(1, &owner(0))
                    .unwrap()
                    .unwrap()
            });
            c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
            let seq = c.sequence();
            TRACE.with_borrow_mut(|t| {
    let t=t.as_mut().unwrap();let mut engine=t.dev.engine.take().unwrap();let baseline=engine.reader().get().unwrap();let disk=failure_recovery::files(&t.dev.home);let mut saved=None;let mut rounds=vec![];
    for replay in 0..=2 {
       if replay>0 {drop(engine);engine=DevEngine::open(&t.dev.home,t.dev.config.clone()).unwrap();}
       let public=engine.account_receipt(seq,&owner(0)).unwrap().unwrap();
       let value=public.to_value();assert_eq!(value["account_result"]["kind"],if correction {"CORRECTION"}else{"SETTLEMENT_APPLY"});
       let field=if correction {"corrected_fill_ids"}else{"committed_fill_ids"};assert!(!value["account_result"][field].as_array().unwrap().is_empty());
       assert!(value["account_result"].get("correction_results").is_none());
       if let Some(saved)=&saved {assert_eq!(public.as_bytes(),saved);}else{saved=Some(public.as_bytes().to_vec());}
       assert_eq!(engine.account_receipt(1,&owner(0)).unwrap().unwrap().as_bytes(),historical.as_bytes());
       assert!(engine.account_receipt(1,&owner(1)).unwrap().is_none());
       let source=engine.trusted_receipt_source(seq).unwrap().unwrap();
       engine.verify_account_receipt(seq,&owner(0),public.as_bytes()).unwrap();
       rounds.push(json!({"replay":replay,"public_base64":STANDARD.encode(public.as_bytes()),"frame_base64":STANDARD.encode(source.frame()),"principal":owner(0),"context":c.snapshot().context()}));
       assert_eq!(engine.reader().get().unwrap().commit,baseline.commit);assert_eq!(failure_recovery::files(&t.dev.home),disk);
    }
    dev_fixture::evidence(&format!("account-event-fee{bps}-correction{correction}"),&json!({"result":"PASS","rounds":rounds,"historical_base64":STANDARD.encode(historical.as_bytes()),"state":baseline.state,"trusted_ledger":baseline.receipts}));
    dev_fixture::copy_home(&format!("account-event-fee{bps}-correction{correction}"),&t.dev.home);
  });
        }
    }
}
#[test]
fn account_missing_evidence_pair_denies_even_earlier_receipt() {
    for bps in [0, 25] {
        let (c, _) = failure_recovery::prepared(bps);
        TRACE.with_borrow_mut(|t| {
      let t=t.as_mut().unwrap();let e=t.dev.engine.take().unwrap();let state=e.reader().get().unwrap();
      let dir=t.dev.home.join("objects/sha256");let p=std::fs::read_dir(&dir).unwrap().map(|e|e.unwrap().path()).find(|p|p.extension().is_none()).unwrap();
      std::fs::remove_file(&p).unwrap();std::fs::remove_file(p.with_extension("ref")).unwrap();let files=failure_recovery::files(&t.dev.home);
      assert!(e.account_receipt(1,&owner(0)).is_err());assert!(e.execute(Command::Apply,&[],&observation(&c),NOW).is_err());
      assert_eq!(e.reader().get().unwrap().commit,state.commit);assert_eq!(e.reader().get().unwrap().state,state.state);drop(e);
      for _ in 0..2 {assert!(DevEngine::open(&t.dev.home,t.dev.config.clone()).is_err());assert_eq!(failure_recovery::files(&t.dev.home),files);}
      dev_fixture::evidence(&format!("account-evidence-missing-fee{bps}"),&json!({"result":"PASS","state_diff":[],"automatic_repair":0,"open_rejections":2,"files":files}));
   });
    }
}

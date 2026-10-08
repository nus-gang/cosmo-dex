//! Offline C command integration; synthetic inputs, no service or broadcast.
#[path = "storage_fault.rs"]
mod storage_fault;
#[path = "../../../exchange/tests/support/dev_fixture.rs"]
mod fixture;
use nus_exchange_contract::s3::{dev_local::{Engine, Validated}, journal::canonical};
use fixture::*;

#[test]
fn scoped_real_command_closes_and_preserves_recovery_evidence() {
    for fee in [0,25] {
        for point in ["before_wal", "before_response"] {
            let (inputs, state)=initial(fee);
            let home=home(fee);
            let engine=Engine::create(&home,Validated::new(inputs.clone()).unwrap(),
                &canonical(&state).unwrap()).unwrap();
            let (raw,sig)=sign_order(&state,0,"2",1000,10000,72);
            let obs=observation(&state);
            let fault=storage_fault::StorageFault::new(point,1,true,true).unwrap();
            let evidence=home.with_extension("fault-report");
            std::fs::create_dir(&evidence).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&evidence,std::fs::Permissions::from_mode(0o700)).unwrap();
            let (out,report)=storage_fault::run_recorded_command(&engine,fault.clone(), &evidence, b"synthetic engine command", ||
                engine.execute(signed(&raw,&sig,0),&[],&obs,NOW)).unwrap();
            assert!(out.is_err());
            assert!(report.injected);
            let evidence_raw=std::fs::read_to_string(evidence.join("storage-fault.jsonl")).unwrap();
            let records:Vec<serde_json::Value>=evidence_raw.lines().map(|line|serde_json::from_str(line).unwrap()).collect();
            assert_eq!(records.len(),2);
            assert_eq!(records[1]["phase"],"scope_returned");
            assert_eq!(records[1]["injected"],true);
            assert_eq!(records[1]["durable_ack"],false);
            let inspector = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ops/s3-local/check_real_fault_report.py");
            let checked = std::process::Command::new("python3").arg(inspector)
                .arg(&evidence)
                .arg(nus_exchange_contract::s3::journal::sha256(b"synthetic engine command"))
                .env("PYTHONDONTWRITEBYTECODE", "1").output().unwrap();
            assert!(checked.status.success(), "{}", String::from_utf8_lossy(&checked.stderr));
            assert_eq!(checked.stdout, b"REAL_WRITER_REPORT_PASS\n");
            assert_eq!(report.matching_visits,1);
            assert_eq!(engine.reader().get().unwrap().gate,"RECOVERY_REQUIRED");
            assert!(engine.execute(signed(&raw,&sig,0),&[],&obs,NOW).is_err());
            assert!(storage_fault::run_command::<()>(&engine,fault, || panic!("reused")).is_err());
            assert!(engine.with_committed_attempt("x",|_,_|panic!("broadcast")).is_err());
            let wal=std::fs::read(home.join("journal.dev.wal")).unwrap();
            let marker=std::fs::read(home.join("commit.dev.json")).unwrap();
            drop(engine);
            for _ in 0..2 {
                let reopened=Engine::open(&home,Validated::new(inputs.clone()).unwrap());
                assert_eq!(reopened.is_ok(),point=="before_response");
                if let Ok(engine)=reopened {
                    let before=engine.reader().get().unwrap();
                    assert_eq!(before.commit.command_seq,1);
                    assert!(engine.execute(signed(&raw,&sig,0),&[],&obs,NOW).unwrap().is_some());
                    let after=engine.reader().get().unwrap();
                    assert_eq!(before.commit,after.commit);
                    assert_eq!(before.state,after.state);
                }
                assert_eq!(wal,std::fs::read(home.join("journal.dev.wal")).unwrap());
                assert_eq!(marker,std::fs::read(home.join("commit.dev.json")).unwrap());
            }
            std::fs::remove_dir_all(home).unwrap();
            std::fs::remove_dir_all(evidence).unwrap();
        }
    }
}

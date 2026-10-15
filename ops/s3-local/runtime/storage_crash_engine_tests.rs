//! Offline real Engine crash/replay. Synthetic keys/pin; no service or RPC.
#[path="storage_crash.rs"] mod storage_crash;
#[path="../../../exchange/tests/support/dev_fixture.rs"] mod fixture;
use fixture::*;
use nus_exchange_contract::s3::{dev_local::{Engine,Validated},journal::{canonical,sha256}};
use std::{os::unix::fs::PermissionsExt,process::{Command,Stdio},time::{Instant,Duration}};
fn command(point:&str)->Vec<u8>{serde_json::to_vec(&serde_json::json!({"effect":"IMMEDIATE_EXIT","exit_code":86,"point":point,"occurrence":"1","command":"synthetic signed Engine order"})).unwrap()}
#[test]
fn engine_crash_child(){
    let Ok(root)=std::env::var("SRE_ENGINE_CRASH_HOME") else{return};
    let fee=std::env::var("SRE_ENGINE_CRASH_FEE").unwrap().parse().unwrap();
    let point=std::env::var("SRE_ENGINE_CRASH_POINT").unwrap();
    let (inputs,state)=initial(fee);let home=std::path::PathBuf::from(root);
    let engine=Engine::open(&home,Validated::new(inputs).unwrap()).unwrap();
    let (raw,sig)=sign_order(&state,0,"2",1000,10000,72);
    let crash=storage_crash::StorageCrash::new(&point,1,true,true).unwrap();
    let _=storage_crash::run_recorded_command(&engine,crash,&home.with_extension("crash"),&command(&point),||engine.execute(signed(&raw,&sig,0),&[],&observation(&state),NOW));
    panic!("crash did not exit");
}
#[test]
fn real_engine_exit_preserves_evidence_and_replay(){
    for fee in [0,25] {for point in ["before_wal","before_response"] {
        let (inputs,state)=initial(fee);let home=home(fee);
        let engine=Engine::create(&home,Validated::new(inputs.clone()).unwrap(),&canonical(&state).unwrap()).unwrap();drop(engine);
        let evidence=home.with_extension("crash");std::fs::create_dir(&evidence).unwrap();
        std::fs::set_permissions(&evidence,std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","engine_crash_child","--nocapture"])
            .env("SRE_ENGINE_CRASH_HOME",&home).env("SRE_ENGINE_CRASH_FEE",fee.to_string()).env("SRE_ENGINE_CRASH_POINT",point)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
        let deadline=Instant::now()+Duration::from_secs(30);
        let status=loop {if let Some(s)=child.try_wait().unwrap(){break s} if Instant::now()>deadline{child.kill().unwrap();child.wait().unwrap();panic!("timeout")}std::thread::sleep(Duration::from_millis(10));};
        assert_eq!(status.code(),Some(86));
        let report=std::fs::read(evidence.join("storage-crash.jsonl")).unwrap();
        assert_eq!(report.iter().filter(|b|**b==b'\n').count(),1);
        let checked=Command::new("python3").arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ops/s3-local/check_real_crash_report.py"))
            .arg(&evidence).arg(sha256(&command(point))).arg("UNKNOWN").env("PYTHONDONTWRITEBYTECODE","1").output().unwrap();
        assert!(checked.status.success(),"{}",String::from_utf8_lossy(&checked.stderr));
        let wal=std::fs::read(home.join("journal.dev.wal")).unwrap();let marker=std::fs::read(home.join("commit.dev.json")).unwrap();
        let mut prior=None;
        for _ in 0..2 {
            let reopened=Engine::open(&home,Validated::new(inputs.clone()).unwrap());
            assert_eq!(reopened.is_ok(),point=="before_response");
            if let Ok(engine)=reopened {
                let before=engine.reader().get().unwrap();assert_eq!(before.commit.command_seq,1);
                let (raw,sig)=sign_order(&state,0,"2",1000,10000,72);
                assert!(engine.execute(signed(&raw,&sig,0),&[],&observation(&state),NOW).unwrap().is_some());
                let after=engine.reader().get().unwrap();assert_eq!(before.commit,after.commit);assert_eq!(before.state,after.state);
                let current=(before.commit.clone(),before.state.clone());if let Some(ref p)=prior{assert_eq!(p,&current)}prior=Some(current);
            }
            assert_eq!(wal,std::fs::read(home.join("journal.dev.wal")).unwrap());assert_eq!(marker,std::fs::read(home.join("commit.dev.json")).unwrap());
            assert_eq!(report,std::fs::read(evidence.join("storage-crash.jsonl")).unwrap());
        }
        std::fs::remove_dir_all(home).unwrap();std::fs::remove_dir_all(evidence).unwrap();
    }}
}

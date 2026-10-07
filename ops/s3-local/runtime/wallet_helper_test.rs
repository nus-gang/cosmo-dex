// Wallet-generated signed TxRaw -> private Go verifier -> one bounded adapter
// send callback. Auth/Account and transport remain synthetic; no chain service.
use super::*;
use std::{fs, os::unix::fs::PermissionsExt};
#[test]
#[ignore = "requires current Wallet fixture and offline Go helper"]
fn wallet_signed_tx_private_helper_boundary() {
 let v:Value=serde_json::from_slice(&fs::read(std::env::var_os("NUS_BROWSER_BOUNDARY").unwrap()).unwrap()).unwrap();
 let root=std::path::PathBuf::from(std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR").unwrap()).canonicalize().unwrap().join(format!("wallet-helper-{}",std::process::id()));
 fs::create_dir(&root).unwrap();fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
 let path=root.join("helper");fs::copy(std::env::var_os("DIRECT_IPC_BINARY").unwrap(),&path).unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o500)).unwrap();
 let hash=nus_exchange_contract::s3::journal::sha256(&fs::read(&path).unwrap());
 let helper=Helper::parse(&mut vec!["--direct-helper".into(),path.to_str().unwrap().into(),"--direct-helper-sha256".into(),hash].into_iter()).unwrap();
 for mode in 0..6 {
  let mut account=v["account"].clone();let mut context=v["context"].clone();
  let mut body:Value=serde_json::from_str(v["body"].as_str().unwrap()).unwrap();
  match mode {
   2=>account["sequence"]=json!((schema::num(&account["sequence"]).unwrap()+1).to_string()),
   3=>account["public_key_base64"]=json!(STANDARD.encode([8;1952])),
   4=>{let mut raw=STANDARD.decode(body["tx_bytes"].as_str().unwrap()).unwrap();let n=raw.len();raw[n-1]^=1;body["tx_bytes"]=json!(STANDARD.encode(raw));},
   5=>{context["genesis_hash"]=json!("fe".repeat(32));account["context"]=context.clone();},
   _=>{}
  }
  let bytes=serde_json::to_vec(&body).unwrap();let headers=[("origin","http://127.0.0.1:5173"),("authorization","Bearer synthetic-session-0")];
  let req=Request{peer:"127.0.0.1".parse().unwrap(),method:"POST",path:"/dev-local/v1/chain/broadcast",headers:&headers,body:&bytes};
  let stop=AtomicBool::new(false);let mut sends=0;let mut verifies=0;
  let projection=json!({"context":context,"owner":v["owner"],"fresh":true,"gate":"OPEN","durable_ack":false});
  let out=broadcast_from(req,&context,&stop,|r|{assert_eq!(r.headers,headers);(200,projection.clone())},|owner|{assert_eq!(owner,schema::bytes(&v["owner"]).unwrap());Ok(account.clone())},
   |r|{verifies+=1;helper.verify(r,&stop).map_err(|_|())},||Ok(schema::num(&account["received_at_unix_ms"]).unwrap()),
   |encoded|{sends+=1;assert_eq!(encoded,body["tx_bytes"].as_str().unwrap());if mode==0 {Ok(json!({"code":0}))}else{Err(())}});
  assert_eq!(verifies,1,"mode {mode}");
  if mode<2 {assert_eq!(out.0,200,"mode {mode}: {:?}",out.1);assert_eq!(out.1["state"],"SUBMISSION_UNKNOWN");assert_eq!(out.1["tx_hash"],v["tx_hash"]);assert_eq!(sends,1);}
  else {assert_eq!(out.0,409,"mode {mode}");assert_eq!(sends,0);}
  helper.recheck().unwrap();
 }
 drop(helper);fs::remove_dir_all(&root).unwrap();assert!(!root.exists());
}

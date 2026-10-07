use super::*;
#[path="../../../exchange/tests/support/dev_fixture.rs"] mod f;
use fips204::{ml_dsa_65, traits::{KeyGen, Signer}};
use nus_exchange_contract::s3::{dev_local::{Engine,Validated},journal::canonical,settlement_local::Options};
use std::{sync::Arc,fs,os::unix::fs::PermissionsExt};
use bech32::ToBase32;
    const ORIGIN: &str = "http://127.0.0.1:5173";
    fn call(rest: &Rest, o: &Observation, path: &str, body: Value, token: Option<&str>) -> (u16, Value) {
        let authorization = format!("Bearer {}",token.unwrap_or(""));
        let mut headers=vec![("origin",ORIGIN)];
        if token.is_some() { headers.push(("authorization",authorization.as_str())); }
        rest.handle(Request {peer:"127.0.0.1".parse().unwrap(),method:"POST",path,headers:&headers,body:&canonical(&body).unwrap()},o,f::NOW)
    }
    fn login(rest: &Rest, o: &Observation, i: usize) -> String {
        let (status,c)=call(rest,o,"/dev-local/v1/auth/challenge",json!({"owner":f::owner(i),"origin":ORIGIN,"audience":"exchange-api"}),None);
        assert_eq!(status,200,"challenge rejected");
        let seed=hex::decode(f::key(i)["test_seed_hex"].as_str().unwrap()).unwrap().try_into().unwrap();
        let (_,sk)=ml_dsa_65::KG::keygen_from_seed(&seed);
        let sig=sk.try_sign_with_seed(&[31;32],&codec::frame("NUS/WALLET_AUTH/V1",&schema::bytes(&c["wire_base64"]).unwrap()),&[]).unwrap();
        let (status,s)=call(rest,o,"/dev-local/v1/auth/session",json!({"wire_base64":c["wire_base64"],"signature_base64":STANDARD.encode(sig)}),None);
        assert_eq!(status,200,"session rejected");
        s["token"].as_str().unwrap().into()
    }

#[test]
#[ignore = "requires explicit compiled Go helper"]
fn real_auth_private_go_rejection_preserves_store() {
 let root=std::path::PathBuf::from(std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR").unwrap()).canonicalize().unwrap().join(format!("broadcast-real-{}",std::process::id()));
 fs::create_dir(&root).unwrap(); fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
 let path=root.join("helper"); fs::copy(std::env::var_os("DIRECT_IPC_BINARY").unwrap(),&path).unwrap();
 fs::set_permissions(&path,fs::Permissions::from_mode(0o500)).unwrap();
 let hash=nus_exchange_contract::s3::journal::sha256(&fs::read(&path).unwrap());
 let helper=Helper::parse(&mut vec!["--direct-helper".into(),path.to_str().unwrap().into(),"--direct-helper-sha256".into(),hash].into_iter()).unwrap();
 for bps in [0,25] {
  let (inputs,initial)=f::initial(bps); let home=f::home(bps);
  let engine=Arc::new(Engine::create(&home,Validated::new(inputs.clone()).unwrap(),&canonical(&initial).unwrap()).unwrap());
  let before=engine.reader().get().unwrap(); let commit=before.commit.clone();
  let anchor=engine.trusted_recovery_history(&commit,None,1).unwrap().latest.snapshot;
  let o=f::observation(&initial);
  let rest=Rest::new(engine.clone(),anchor.clone(),Options{enabled:true,acknowledge_unproven_space:true,bind:"127.0.0.1".parse().unwrap()}).unwrap();
  for i in 0..2 {
   let token=login(&rest,&o,i); let authorization=format!("Bearer {token}");
   let headers=[("origin",ORIGIN),("authorization",authorization.as_str())];
   let req=||Request{peer:"127.0.0.1".parse().unwrap(),method:"POST",path:"/dev-local/v1/chain/broadcast",headers:&headers,body:br#"{"tx_bytes":"YWJj"}"#};
   let stop=AtomicBool::new(false); let mut queries=0; let mut verifies=0;
   let out=broadcast_from(req(),anchor.context(),&stop,|r|rest.handle(r,&o,f::NOW),|owner|{
    queries+=1; assert_eq!(owner,schema::bytes(&json!(f::owner(i))).unwrap());
    Ok(json!({"context":anchor.context(),"owner":bech32::encode("nus",owner.to_base32(),bech32::Variant::Bech32).unwrap(),"public_key_base64":STANDARD.encode([8;1952]),"account_number":"0","sequence":"0","received_at_unix_ms":f::NOW.to_string()}))
   },|r|{verifies+=1;assert_eq!(r.raw,b"abc");helper.verify(r,&stop).map_err(|_|())},||Ok(f::NOW),|_|panic!("invalid TX broadcast"));
   assert_eq!(out.0,409);assert_eq!((queries,verifies),(1,1));helper.recheck().unwrap();
   assert_eq!(call(&rest,&o,"/dev-local/v1/auth/logout",json!({}),Some(&token)).0,200);
   assert_eq!(broadcast_from(req(),anchor.context(),&stop,|r|rest.handle(r,&o,f::NOW),|_|panic!("query after logout"),|_|panic!("helper after logout"),||panic!("clock"),|_|panic!("send")).0,401);
  }
  assert_eq!(engine.reader().get().unwrap().commit,commit);
  drop(before);drop(rest);drop(engine);
  for _ in 0..2 {let e=Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap();assert_eq!(e.reader().get().unwrap().commit,commit);drop(e);}
  fs::remove_dir_all(home).unwrap();
 }
 drop(helper);fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "explicit offline Go helper and installed Node required"]
fn real_auth_wallet_signed_helper_owner_isolation() {
 let root=std::path::PathBuf::from(std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR").unwrap()).canonicalize().unwrap().join(format!("broadcast-signed-{}",std::process::id()));
 fs::create_dir(&root).unwrap();fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
 let path=root.join("helper");fs::copy(std::env::var_os("DIRECT_IPC_BINARY").unwrap(),&path).unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o500)).unwrap();
 let hash=nus_exchange_contract::s3::journal::sha256(&fs::read(&path).unwrap());
 let helper=Helper::parse(&mut vec!["--direct-helper".into(),path.to_str().unwrap().into(),"--direct-helper-sha256".into(),hash].into_iter()).unwrap();
 for bps in [0,25] {
  let (inputs,initial)=f::initial(bps);let home=f::home(bps);
  let engine=Arc::new(Engine::create(&home,Validated::new(inputs.clone()).unwrap(),&canonical(&initial).unwrap()).unwrap());
  let before=engine.reader().get().unwrap();let commit=before.commit.clone();
  let anchor=engine.trusted_recovery_history(&commit,None,1).unwrap().latest.snapshot;
  let o=f::observation(&initial);
  let rest=Rest::new(engine.clone(),anchor.clone(),Options{enabled:true,acknowledge_unproven_space:true,bind:"127.0.0.1".parse().unwrap()}).unwrap();
  let tokens=[login(&rest,&o,0),login(&rest,&o,1)];
  for i in 0..2 { for operation in ["DEPOSIT","WITHDRAW"] {
   let input=root.join(format!("{bps}-{i}-{operation}.in.json"));let output=input.with_extension("out.json");
   fs::write(&input,serde_json::to_vec(&json!({"context":anchor.context(),"index":i,"operation":operation})).unwrap()).unwrap();
   let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("ops/s3-local/fixture-direct.ts");
   assert!(std::process::Command::new("node").args(["--experimental-strip-types"]).arg(script).arg(input).arg(&output).status().unwrap().success());
   let tx:Value=serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
   let body=canonical(&json!({"tx_bytes":tx["tx_bytes"]})).unwrap();
   for mode in 0..3 {
    let auth_i=if mode==2 {1-i}else{i};let authorization=format!("Bearer {}",tokens[auth_i]);
    let headers=[("origin",ORIGIN),("authorization",authorization.as_str())];
    let req=Request{peer:"127.0.0.1".parse().unwrap(),method:"POST",path:"/dev-local/v1/chain/broadcast",headers:&headers,body:&body};
    let stop=AtomicBool::new(false);let mut sends=0;let mut verifies=0;
    let out=broadcast_from(req,anchor.context(),&stop,|r|rest.handle(r,&o,f::NOW),|owner|{
     assert_eq!(owner,schema::bytes(&json!(f::owner(auth_i))).unwrap());
     Ok(json!({"context":anchor.context(),"owner":bech32::encode("nus",owner.to_base32(),bech32::Variant::Bech32).unwrap(),"public_key_base64":STANDARD.encode(hex::decode(f::key(auth_i)["public_key_hex"].as_str().unwrap()).unwrap()),"account_number":"0","sequence":"0","received_at_unix_ms":f::NOW.to_string()}))
    },|r|{verifies+=1;helper.verify(r,&stop).map_err(|_|())},||Ok(f::NOW),|encoded|{sends+=1;assert_eq!(encoded,tx["tx_bytes"]);if mode==0 {Ok(json!({}))}else{Err(())}});
    assert_eq!(verifies,1);
    if mode==2 {assert_eq!(out.0,409);assert_eq!(sends,0);}else{assert_eq!(out.0,200,"{:?}",out.1);assert_eq!(out.1["tx_hash"],tx["tx_hash"]);assert_eq!(out.1["state"],"SUBMISSION_UNKNOWN");assert_eq!(sends,1);}
   }
  }}
  for token in &tokens {
   assert_eq!(call(&rest,&o,"/dev-local/v1/auth/logout",json!({}),Some(token)).0,200);
   let auth=format!("Bearer {token}");let headers=[("origin",ORIGIN),("authorization",auth.as_str())];
   let req=Request{peer:"127.0.0.1".parse().unwrap(),method:"POST",path:"/dev-local/v1/chain/broadcast",headers:&headers,body:br#"{"tx_bytes":"YWJj"}"#};
   assert_eq!(broadcast_from(req,anchor.context(),&AtomicBool::new(false),|r|rest.handle(r,&o,f::NOW),|_|panic!("query after logout"),|_|panic!("helper"),||panic!("clock"),|_|panic!("send")).0,401);
  }
  assert_eq!(engine.reader().get().unwrap().commit,commit);assert_eq!(engine.reader().get().unwrap().state,before.state);
  drop(before);drop(rest);drop(engine);
  for _ in 0..2 {let e=Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap();assert_eq!(e.reader().get().unwrap().commit,commit);drop(e);}
  fs::remove_dir_all(home).unwrap();
 }
 drop(helper);fs::remove_dir_all(&root).unwrap();assert!(!root.exists());
}

//! Authenticated direct-TX adapter. Trusted Account query and offline signature
//! verification precede one broadcast. No automatic retry or finality claim.
#[path="collect.rs"] pub(super) mod collect;
#[path="chain_http.rs"] pub(super) mod chain_http;
#[path="direct_helper.rs"] mod direct_helper;
#[path="chain_result.rs"] pub(super) mod chain_result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nus_exchange_contract::{codec, s3::{schema, dev_local, settlement_local::{Request,Rest}, snapshot::{Observation,Snapshot}}};
use serde_json::{Value,json};
use std::sync::atomic::{AtomicBool,Ordering};
pub use direct_helper::Helper;
pub use collect::ChainRead;
fn rejected()->(u16,Value) {(409,json!({"code":"CHAIN_BROADCAST_REJECTED","durable_ack":false}))}

pub fn broadcast(rest:&Rest, req:Request<'_>, observation:&Observation, anchor:&Snapshot,
    chain:&ChainRead, helper:&Helper, stop:&AtomicBool,
    clock:impl FnMut()->dev_local::Result<u64>) -> (u16,Value) {
    let clock=std::cell::RefCell::new(clock);
    broadcast_from(req,anchor.context(),stop,
        |r| match (clock.borrow_mut())() { Ok(now)=>rest.handle(r,observation,now),Err(_)=>rejected() },
        |owner|chain.direct_account(anchor,owner,|| (clock.borrow_mut())()).map(|(v,_)|v).map_err(|_|()),
        |r|helper.verify(r,stop).map_err(|_|()),
        ||(clock.borrow_mut())().map_err(|_|()),
        |encoded|chain.direct_broadcast(encoded).map_err(|_|()))
}
fn broadcast_from<'a>(req:Request<'a>, context:&Value, stop:&AtomicBool,
    mut auth:impl FnMut(Request<'a>)->(u16,Value),
    query:impl FnOnce(&[u8])->Result<Value,()>,
    verify:impl FnOnce(&direct_helper::Request<'_>)->Result<(),()>,
    clock:impl FnOnce()->Result<u64,()>,
    send:impl FnOnce(&str)->Result<Value,()>) -> (u16,Value) {
    if req.method!="POST" || req.path!="/dev-local/v1/chain/broadcast" || req.body.len()>192*1024 {
        return (400,json!({"code":"CHAIN_REQUEST_REJECTED","durable_ack":false}));
    }
    let body=match codec::unique_json(req.body) {Ok(v)=>v,Err(_)=>return rejected()};
    let Some(obj)=body.as_object() else{return rejected()};
    if obj.len()!=1 {return rejected()}
    let Some(encoded)=body["tx_bytes"].as_str() else{return rejected()};
    if encoded.is_empty() || encoded.len()>139264_usize.div_ceil(3)*4 {return rejected()}
    let raw=match STANDARD.decode(encoded) {Ok(v) if !v.is_empty() && v.len()<=139264 && STANDARD.encode(&v)==encoded=>v,_=>return rejected()};
    let request=||Request{peer:req.peer,method:"GET",path:"/dev-local/v1/chain/account",headers:req.headers,body:b""};
    if stop.load(Ordering::Relaxed) {return rejected()}
    let (status,account)=chain_http::account_from(request(),context,&mut auth,query);
    if status!=200 {return (status,account)}
    let result=(||->Result<(),()> {
        let owner=codec::decode_address(account["owner"].as_str().ok_or(())?).map_err(|_|())?;
        let pk=schema::bytes(&account["public_key_base64"]).map_err(|_|())?;
        let genesis=hex::decode(context["genesis_hash"].as_str().ok_or(())?).map_err(|_|())?;
        verify(&direct_helper::Request{raw:&raw,owner:&owner,public_key:&pk,genesis:&genesis,
            chain_id:context["chain_id"].as_str().ok_or(())?,
            account_number:schema::num(&account["account_number"]).map_err(|_|())?,
            sequence:schema::num(&account["sequence"]).map_err(|_|())?})?;
        let now=clock()?; let received=schema::num(&account["received_at_unix_ms"]).map_err(|_|())?;
        if now<received || now-received>2000 || stop.load(Ordering::Relaxed) {return Err(())}
        // Rest rechecks the original observation, session, origin and gate after
        // the bounded child. Do not refresh freshness by replacing its timestamps.
        let (status,again)=chain_http::account_from(request(),context,&mut auth,|_|Ok(account.clone()));
        if status!=200 || again!=account || stop.load(Ordering::Relaxed) {return Err(())}
        Ok(())
    })();
    if result.is_err() {return rejected()}
    // Once transport is attempted, every result is unknown, including IO errors.
    let _=send(encoded);
    (200,json!({"tx_hash":nus_exchange_contract::s3::journal::sha256(&raw),"state":"SUBMISSION_UNKNOWN"}))
}
#[cfg(test)] mod broadcast_tests {
 use super::*; use bech32::ToBase32;
 fn context()->Value {json!({"chain_id":"nus-s3-dev-1","genesis_hash":"aa".repeat(32)})}
 fn projection()->Value {json!({"context":context(),"owner":STANDARD.encode([7;20]),"fresh":true,"gate":"OPEN","durable_ack":false})}
 fn account()->Value {json!({"context":context(),"owner":bech32::encode("nus",[7;20].to_base32(),bech32::Variant::Bech32).unwrap(),"public_key_base64":STANDARD.encode([8;1952]),"account_number":"9","sequence":"10","received_at_unix_ms":"1000"})}
 fn req(body:&[u8])->Request<'_> {Request{peer:"127.0.0.1".parse().unwrap(),method:"POST",path:"/dev-local/v1/chain/broadcast",headers:&[("origin","http://127.0.0.1:5173"),("authorization","Bearer test")],body}}
 #[test] #[ignore = "invoked by Wallet roundtrip with explicit request/response files"] fn wallet_three_route_bridge() {
  let path=std::env::var_os("NUS_ROUTE_INPUT").expect("explicit Wallet request file");
  let v:Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
  let header=v["authorization"].as_str().unwrap();
  let headers=[("authorization",header),("origin","http://127.0.0.1:5173")];
  let body=v["body"].as_str().unwrap().as_bytes();
  let req=Request{peer:"127.0.0.1".parse().unwrap(),method:v["method"].as_str().unwrap(),path:v["path"].as_str().unwrap(),headers:&headers,body};
  let p=json!({"context":v["context"],"owner":v["owner"],"fresh":true,"gate":"OPEN","durable_ack":false});
  let auth=|r:Request<'_>| {assert_eq!(r.headers,headers);assert_eq!(r.path,"/dev-local/v1/account");assert_eq!(header,"Bearer synthetic-session-0");(200,p.clone())};
  let sends=std::cell::Cell::new(0);let stop=AtomicBool::new(false);
  let out=match req.path {
   "/dev-local/v1/chain/account"=>chain_http::account_from(req,&v["context"],auth,|owner|{assert_eq!(owner,schema::bytes(&v["owner"]).unwrap());Ok(v["account"].clone())}),
   "/dev-local/v1/chain/broadcast"=>broadcast_from(req,&v["context"],&stop,auth,|_|Ok(v["account"].clone()),|r|{assert_eq!(r.owner,schema::bytes(&v["owner"]).unwrap());Ok(())},||Ok(schema::num(&v["account"]["received_at_unix_ms"]).unwrap()),|_|{sends.set(sends.get()+1);Err(())}),
   "/dev-local/v1/chain/result"=>chain_result::result_from(req,&v["context"],&stop,auth,|hash|{assert_eq!(hash,v["result"]["tx_hash"].as_str().unwrap());if v["query_error"]==true {Err(())}else{Ok(v["result"].clone())}}),
   _=>panic!("unexpected route")
  };
  std::fs::write(std::env::var_os("NUS_ROUTE_OUTPUT").unwrap(),serde_json::to_vec(&json!({"status":out.0,"body":out.1,"sends":sends.get()})).unwrap()).unwrap();
 }
 #[test] fn approved_wallet_body_crosses_adapter_once() {
  let path=std::env::var("NUS_BROWSER_BOUNDARY").expect("generated Wallet boundary fixture");
  let v:Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
  let body=v["body"].as_str().unwrap().as_bytes();
  let account=v["account"].clone();let now=schema::num(&account["received_at_unix_ms"]).unwrap();
  let projection=json!({"context":v["context"],"owner":v["owner"],"fresh":true,"gate":"OPEN","durable_ack":false});
  for transport_ok in [false,true] {
   let sends=std::cell::Cell::new(0);let verifies=std::cell::Cell::new(0);
   let out=broadcast_from(req(body),&v["context"],&AtomicBool::new(false),
    |_|(200,projection.clone()), |_|Ok(account.clone()), |request|{
      verifies.set(verifies.get()+1);
      assert_eq!(nus_exchange_contract::s3::journal::sha256(request.raw),v["tx_hash"]);
      assert_eq!(request.owner,schema::bytes(&v["owner"]).unwrap());Ok(())},
    ||Ok(now), |encoded|{sends.set(sends.get()+1);
      assert_eq!(encoded,serde_json::from_slice::<Value>(body).unwrap()["tx_bytes"]);
      if transport_ok{Ok(json!({}))}else{Err(())}});
   assert_eq!(out.0,200);assert_eq!(out.1["tx_hash"],v["tx_hash"]);
   assert_eq!(out.1["state"],"SUBMISSION_UNKNOWN");assert_eq!(sends.get(),1);assert_eq!(verifies.get(),1);
  }
 }
 #[test] fn exact_owner_verified_before_one_unknown_send() {
  for transport_ok in [true,false] {
   let stop=AtomicBool::new(false); let calls=std::cell::RefCell::new(vec![]);
   let out=broadcast_from(req(br#"{"tx_bytes":"YWJj"}"#),&context(),&stop,
    |r|{calls.borrow_mut().push("auth");assert_eq!(r.headers.len(),2);assert_eq!(r.path,"/dev-local/v1/account");(200,projection())},
    |owner|{calls.borrow_mut().push("query");assert_eq!(owner,[7;20]);Ok(account())},
    |r|{calls.borrow_mut().push("verify");assert_eq!(r.raw,b"abc");assert_eq!(r.owner,[7;20]);assert_eq!(r.public_key,[8;1952]);assert_eq!(r.genesis,[0xaa;32]);assert_eq!((r.account_number,r.sequence),(9,10));Ok(())},
    ||Ok(3000),|s|{calls.borrow_mut().push("send");assert_eq!(s,"YWJj");if transport_ok{Ok(json!({"state":"COMMITTED"}))}else{Err(())}});
   assert_eq!(out.0,200);assert_eq!(out.1["state"],"SUBMISSION_UNKNOWN");
   assert_eq!(*calls.borrow(),["auth","query","verify","auth","send"]);
  }
 }
 #[test] fn helper_failure_stale_revocation_stop_never_send() {
  for mode in 0..6 {
   let stop=AtomicBool::new(false);let mut count=0;
   let out=broadcast_from(req(br#"{"tx_bytes":"YWJj"}"#),&context(),&stop,
    |_|{count+=1;if mode==3 && count==2 {(401,json!({}))}else{(200,projection())}},
    |_|Ok(account()), |_|{if mode==4 {stop.store(true,Ordering::Relaxed)} if mode==0{Err(())}else{Ok(())}},
    ||match mode {1=>Ok(3001),2=>Ok(999),5=>Err(()),_=>Ok(1000)}, |_|panic!("broadcast"));
   assert_eq!(out.0,409);
  }
 }
 #[test] fn invalid_body_and_auth_cannot_reach_helper() {
  let stop=AtomicBool::new(false);
  for b in [r#"{"tx_base64":"YQ=="}"#,r#"{"tx_bytes":"YQ"}"#,r#"{"tx_bytes":"YQ==","owner":"x"}"#,r#"{"tx_bytes":"YQ==","tx_bytes":"Yg=="}"#,r#"[]"#] {
   assert_eq!(broadcast_from(req(b.as_bytes()),&context(),&stop,|_|panic!("auth"),|_|panic!("query"),|_|panic!("verify"),||panic!("clock"),|_|panic!("send")).0,409);
  }
  assert_eq!(broadcast_from(req(br#"{"tx_bytes":"YQ=="}"#),&context(),&stop,|_|(401,json!({"code":"AUTH"})),|_|panic!("query"),|_|panic!("verify"),||panic!("clock"),|_|panic!("send")).0,401);
 }
}

#[cfg(test)]
#[path="chain_broadcast_real_test.rs"] mod real_broadcast_tests;

#[cfg(test)]
#[path="wallet_helper_test.rs"] mod wallet_helper_tests;

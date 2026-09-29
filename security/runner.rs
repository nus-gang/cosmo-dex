// Test-only adapter; production Rust component exposes verification, not signing.
use std::io::{self,BufRead};
use serde_json::{Value,json};
use nus_exchange_contract::{codec::{Codec,frame,verify_raw},policy::{self,OrderContext}};
use fips204::{ml_dsa_65,traits::{KeyGen,Signer,SerDes}};
fn s<'a>(r:&'a Value,k:&str)->&'a str{r[k].as_str().unwrap_or("")}
fn h(r:&Value,k:&str)->Vec<u8>{hex::decode(s(r,k)).unwrap()}
fn run(r:&Value)->Result<Value,&'static str>{let mut o=json!({"code":"OK"});match s(r,"Op"){
"sign"=>{let seed:[u8;32]=h(r,"Seed").try_into().unwrap();let(pk,sk)=ml_dsa_65::KG::keygen_from_seed(&seed);o["pk"]=json!(hex::encode(pk.into_bytes()));o["sig"]=json!(hex::encode(sk.try_sign_with_seed(&[0;32],&h(r,"Msg"),&h(r,"Context"))?));},
"verify"=>o["valid"]=json!(verify_raw(&h(r,"PK"),&h(r,"Msg"),&h(r,"Sig"),&h(r,"Context"))),
"encode"=>{let b=Codec::default().encode(s(r,"Name"),&r["API"])?;o["wire"]=json!(hex::encode(&b));o["msg"]=json!(hex::encode(frame(s(r,"Domain"),&b)));},
"decode"=>o["api"]=Codec::default().decode(s(r,"Name"),&h(r,"Wire"))?,
"fee"=>o["fee"]=json!(policy::fee(s(r,"Receive").parse().unwrap(),r["BPS"].as_u64().unwrap() as u32)?.to_string()),
"policy"=>{let a=&r["API"];let pk=if r["RegisteredKey"].is_string(){h(r,"RegisteredKey")}else{h(r,"PK")};let expected=if r["Expected"].is_object(){&r["Expected"]}else{a};let field=|k|expected[k].as_str().unwrap_or(s(a,k));let c=OrderContext{chain_id:field("chain_id"),genesis_hash:field("genesis_hash"),exchange_module_id:field("exchange_module_id"),market_id:field("market_id"),market_config_version:s(a,"market_config_version").parse().unwrap(),registered_key:Some(&pk),epoch:s(a,"owner_epoch").parse().unwrap(),height:r["Height"].as_u64().unwrap_or(999),revoked:false,filled:0,available:u128::MAX,fee_bps:r["BPS"].as_u64().unwrap_or(0) as u32};policy::validate_order(&h(r,"Wire"),&h(r,"Sig"),&c)?;},
_=>return Err("unsupported")};Ok(o)}
fn main(){for line in io::stdin().lock().lines(){let r:Value=serde_json::from_str(&line.unwrap()).unwrap();println!("{}",run(&r).unwrap_or_else(|e|json!({"code":e})));}}

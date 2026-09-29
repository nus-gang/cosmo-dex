use nus_exchange_contract::{codec::*,decision::*};
use serde_json::{Value,json};
use std::io::{self,Read};
fn h(v:&Value)->Vec<u8>{hex::decode(v.as_str().unwrap()).unwrap()}
fn run(v:&Value)->Result<Value,&'static str>{
 let codec=Codec::default();
 match v["op"].as_str().unwrap(){
 "crypto"=>Ok(json!(verify_raw(&h(&v["pk"]),&h(&v["input"]),&h(&v["signature"]),&hex::decode(v["context"].as_str().unwrap_or("")).unwrap()))),
 "frame"=>Ok(json!(hex::encode(frame(v["domain"].as_str().unwrap(),&h(&v["wire"]))))),
 "atoms"=>Ok(json!(hex::encode(integer(&v["api"],128)?.to_be_bytes()))),
 "atoms_decode"=>{let b=h(&v["wire"]);if b.len()!=16{return Err("NON_CANONICAL_WIRE")};Ok(json!(u128::from_be_bytes(b.try_into().unwrap()).to_string()))},
 "encode"=>Ok(json!(hex::encode(codec.encode(v["message"].as_str().unwrap(),&v["api"])?))),
 "decode"=>{codec.decode(v["message"].as_str().unwrap(),&h(&v["wire"]))?;Ok(json!("CANONICAL"))},
 "fee"=>Ok(json!(fee_json(&v["receive"],&v["rate"])?)),
 "cap"=>{cap_check(&v["cap"],&v["rate"])?;Ok(json!("OK"))},
 "decision"=>Ok(evaluate_snapshot(v["auth"].clone(),&v["snapshot"])),
 "api"=>api_error(v["code"].as_str().unwrap(),None).ok_or("UNKNOWN_API_ERROR"),
 _=>Err("UNSUPPORTED_OPERATION")
 }
}
fn main(){let mut s=String::new();io::stdin().read_to_string(&mut s).unwrap();let a:Vec<Value>=serde_json::from_str(&s).unwrap();
 let out:Vec<Value>=a.iter().map(|v|json!({"id":v["id"],"actual":run(v).unwrap_or_else(|e|json!(e))})).collect();println!("{}",json!(out));}

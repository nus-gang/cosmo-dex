//! JSON-lines independent re-test port; contexts/snapshots are trusted harness inputs only.
use nus_exchange_contract::{Result, codec::integer, decision::*, policy::OrderContext};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn string<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or("NOT_CONNECTED")
}
fn number(v: &Value, k: &str, bits: u32) -> Result<u128> {
    integer(&v[k], bits)
}
fn run(v: &Value) -> Result<Value> {
    match string(v, "op")? {
        "fee" => Ok(json!({"result":fee_json(&v["receive"],&v["active_bps"])?})),
        "cap" => {
            cap_check(&v["cap"], &v["active_bps"])?;
            Ok(json!({"result":"OK"}))
        }
        "admit_order" => {
            let c = &v["context"];
            let raw = hex::decode(string(v, "wire_hex")?).map_err(|_| "NON_CANONICAL")?;
            let sig = hex::decode(string(v, "signature_hex")?).map_err(|_| "INVALID_SIGNATURE")?;
            // An omitted registration is disconnected; explicit null is unregistered.
            if !c.as_object().is_some_and(|c| c.contains_key("registered")) {
                return Ok(evaluate_snapshot(
                    stage(Err("NOT_CONNECTED")),
                    &v["snapshot"],
                ));
            }
            let reg = &c["registered"];
            let pk = if reg.is_null() {
                None
            } else {
                Some(hex::decode(string(reg, "raw_key_hex")?).map_err(|_| "KEY_LENGTH")?)
            };
            let ctx = OrderContext {
                snapshot_id: string(c, "snapshot_id")?,
                chain_id: string(c, "chain_id")?,
                genesis_hash: string(c, "genesis_hash")?,
                exchange_module_id: string(c, "exchange_module_id")?,
                market_id: string(c, "market_id")?,
                market_config_version: number(c, "market_config_version", 64)? as u64,
                registered_key: pk.as_deref(),
                registered_key_type: reg["key_type"].as_str(),
                height: number(c, "height", 64)? as u64,
                // These legacy validate_order fields are not used by authenticate_order.
                epoch: 0,
                revoked: false,
                filled: 0,
                available: 0,
                fee_bps: 0,
            };
            Ok(admit_order(&raw, &sig, &ctx, &v["snapshot"]))
        }
        _ => Err("UNSUPPORTED_OPERATION"),
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let out = line
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .ok_or("NON_CANONICAL")
            .and_then(|v| run(&v));
        println!("{}", out.unwrap_or_else(|e| json!({"error":e})));
    }
}

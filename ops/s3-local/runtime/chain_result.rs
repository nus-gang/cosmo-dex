//! Authenticated, bounded finality projection; never changes engine state.
use super::collect::ChainRead;
use nus_exchange_contract::{
    codec,
    s3::{
        dev_local, schema,
        settlement_local::{Request, Rest},
        snapshot::{Observation, Snapshot},
    },
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
fn rejected() -> (u16, Value) {
    (
        409,
        json!({"code":"CHAIN_RESULT_UNAVAILABLE","durable_ack":false}),
    )
}
pub fn result(
    rest: &Rest,
    req: Request<'_>,
    observation: &Observation,
    anchor: &Snapshot,
    chain: &ChainRead,
    stop: &AtomicBool,
    mut clock: impl FnMut() -> dev_local::Result<u64>,
) -> (u16, Value) {
    result_from(
        req,
        anchor.context(),
        stop,
        |r| match clock() {
            Ok(now) => rest.handle(r, observation, now),
            Err(_) => rejected(),
        },
        |hash| {
            chain
                .direct_result(anchor, hash)
                .map(|(v, _)| v)
                .map_err(|_| ())
        },
    )
}
pub(super) fn result_from<'a>(
    req: Request<'a>,
    context: &Value,
    stop: &AtomicBool,
    mut auth: impl FnMut(Request<'a>) -> (u16, Value),
    query: impl FnOnce(&str) -> Result<Value, ()>,
) -> (u16, Value) {
    if req.method != "POST" || req.path != "/dev-local/v1/chain/result" || req.body.len() > 128 {
        return (
            400,
            json!({"code":"CHAIN_REQUEST_REJECTED","durable_ack":false}),
        );
    }
    let Ok(body) = codec::unique_json(req.body) else {
        return rejected();
    };
    let Some(hash) = body["tx_hash"].as_str() else {
        return rejected();
    };
    if body.as_object().map(|o| o.len()) != Some(1)
        || hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return rejected();
    }
    let request = || Request {
        peer: req.peer,
        method: "GET",
        path: "/dev-local/v1/account",
        headers: req.headers,
        body: b"",
    };
    let valid = |p: &Value| {
        p["context"] == *context
            && p["fresh"] == true
            && p["gate"] == "OPEN"
            && p["durable_ack"] == false
            && schema::bytes(&p["owner"]).is_ok_and(|v| v.len() == 20)
    };
    if stop.load(Ordering::Relaxed) {
        return rejected();
    }
    let (status, p) = auth(request());
    if status != 200 {
        return (status, p);
    }
    if !valid(&p) {
        return rejected();
    }
    let Ok(v) = query(hash) else {
        return rejected();
    };
    // The trusted collector proves inclusion. Recheck its projection and never
    // expose unrelated snapshot/accounts or evidence objects through HTTP.
    let checked = (|| -> Result<Value, ()> {
        let raw = schema::bytes(&v["tx_bytes"]).map_err(|_| ())?;
        let height = schema::num(&v["height"]).map_err(|_| ())?;
        let code = schema::num(&v["code"]).map_err(|_| ())?;
        let state = if code == 0 {
            "COMMITTED"
        } else {
            "REJECTED_FINAL"
        };
        if raw.is_empty()
            || raw.len() > 139264
            || height == 0
            || v["context"] != *context
            || v["tx_hash"] != hash
            || nus_exchange_contract::s3::journal::sha256(&raw) != hash
            || v["state"] != state
        {
            return Err(());
        }
        Ok(
            json!({"context":context,"tx_hash":hash,"tx_bytes":v["tx_bytes"],"height":height.to_string(),"code":code.to_string(),"state":state}),
        )
    })();
    let Ok(view) = checked else { return rejected() };
    let (status, again) = auth(request());
    if status != 200
        || !valid(&again)
        || again["owner"] != p["owner"]
        || stop.load(Ordering::Relaxed)
    {
        return rejected();
    }
    (200, view)
}
#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    fn projection() -> Value {
        json!({"context":{"test":true},"owner":STANDARD.encode([7;20]),"fresh":true,"gate":"OPEN","durable_ack":false})
    }
    fn value(code: u64) -> Value {
        json!({"context":{"test":true},"tx_hash":nus_exchange_contract::s3::journal::sha256(b"tx"),"tx_bytes":STANDARD.encode(b"tx"),"height":"2","code":code.to_string(),"state":if code==0{"COMMITTED"}else{"REJECTED_FINAL"},"private":"must not escape"})
    }
    fn body() -> Vec<u8> {
        serde_json::to_vec(&json!({"tx_hash":value(0)["tx_hash"]})).unwrap()
    }
    fn req(b: &[u8]) -> Request<'_> {
        Request {
            peer: "127.0.0.1".parse().unwrap(),
            method: "POST",
            path: "/dev-local/v1/chain/result",
            headers: &[("Authorization", "x"), ("Authorization", "y")],
            body: b,
        }
    }
    #[test]
    fn exact_hash_projection_and_two_auth_checks() {
        for code in [0, 1019] {
            let mut calls = 0;
            let v = value(code);
            let p = projection();
            let out = result_from(
                req(&body()),
                &p["context"],
                &AtomicBool::new(false),
                |r| {
                    calls += 1;
                    assert_eq!(r.headers.len(), 2);
                    assert_eq!(r.path, "/dev-local/v1/account");
                    (200, p.clone())
                },
                |hash| {
                    assert_eq!(hash, v["tx_hash"].as_str().unwrap());
                    Ok(v.clone())
                },
            );
            assert_eq!(out.0, 200);
            assert_eq!(out.1["state"], v["state"]);
            assert!(out.1.get("private").is_none());
            assert_eq!(calls, 2);
        }
    }
    #[test]
    fn malformed_and_unauthenticated_do_not_query() {
        for b in [
            b"{}".as_slice(),
            br#"{"tx_hash":"AB"}"#,
            br#"{"tx_hash":"a","tx_hash":"b"}"#,
        ] {
            assert_eq!(
                result_from(
                    req(b),
                    &json!({}),
                    &AtomicBool::new(false),
                    |_| panic!("auth"),
                    |_| panic!("IO")
                )
                .0,
                409
            );
        }
        let p = projection();
        assert_eq!(
            result_from(
                req(&body()),
                &p["context"],
                &AtomicBool::new(false),
                |_| (401, json!({"code":"AUTH"})),
                |_| panic!("IO")
            )
            .0,
            401
        );
    }
    #[test]
    fn unknown_tamper_revocation_and_stop_never_claim_finality() {
        for mode in 0..8 {
            let p = projection();
            let stop = AtomicBool::new(false);
            let mut calls = 0;
            let out = result_from(
                req(&body()),
                &p["context"],
                &stop,
                |_| {
                    calls += 1;
                    let mut p = p.clone();
                    if calls == 2 {
                        match mode {
                            5 => return (401, json!({})),
                            6 => p["owner"] = json!(STANDARD.encode([8; 20])),
                            _ => (),
                        }
                    }
                    (200, p)
                },
                |_| {
                    let mut v = value(0);
                    match mode {
                        0 => return Err(()),
                        1 => v["tx_bytes"] = json!("YQ=="),
                        2 => v["context"] = json!({}),
                        3 => v["state"] = json!("REJECTED_FINAL"),
                        4 => v["height"] = json!("0"),
                        7 => stop.store(true, Ordering::Relaxed),
                        _ => (),
                    }
                    Ok(v)
                },
            );
            assert_eq!(out, rejected());
        }
    }
}

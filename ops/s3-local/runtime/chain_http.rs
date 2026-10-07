//! SRE read-only ChainPort HTTP adapter. Reuses the existing REST session store.
//! No second authentication store, signer, listener or browser-selected owner.
use nus_exchange_contract::s3::{settlement_local::{Request, Rest}, snapshot::{Observation, Snapshot}};
use serde_json::{Value, json};
use super::collect::ChainRead;

fn rejected() -> (u16, Value) {
    (409, json!({"code":"CHAIN_ACCOUNT_UNAVAILABLE","durable_ack":false}))
}

/// Caller supplies the same validated Rest/anchor as worker startup, and wall
/// time sampled after request read. A separate fresh Chain query retains its
/// own original observation times. Raw evidence stays internal to ChainRead.
pub fn account(rest: &Rest, req: Request<'_>, observation: &Observation,
    now: u64, anchor: &Snapshot, chain: &ChainRead,
    mut clock: impl FnMut() -> nus_exchange_contract::s3::dev_local::Result<u64>,
) -> (u16, Value) {
    account_from(req, anchor.context(), |auth_req| rest.handle(auth_req, observation, now),
        |owner| chain.direct_account(anchor, owner, &mut clock).map(|(v, _)| v).map_err(|_| ()))
}

pub(super) fn account_from<'a>(req: Request<'a>, context: &Value,
    authenticate: impl FnOnce(Request<'a>) -> (u16, Value),
    query: impl FnOnce(&[u8]) -> Result<Value, ()>,
) -> (u16, Value) {
    if req.method != "GET" || req.path != "/dev-local/v1/chain/account" || !req.body.is_empty() {
        return (400, json!({"code":"CHAIN_REQUEST_REJECTED","durable_ack":false}));
    }
    // Preserve actual peer and ALL headers (including duplicates). Only the
    // fixed internal route changes; existing Rest owns origin/session checks.
    let (status, projection) = authenticate(Request { path: "/dev-local/v1/account", ..req });
    if status != 200 { return (status, projection); }
    if projection["context"] != *context || projection["fresh"] != true
        || projection["gate"] != "OPEN" || projection["durable_ack"] != false {
        return rejected();
    }
    let Ok(bytes) = nus_exchange_contract::s3::schema::bytes(&projection["owner"]) else { return rejected() };
    if bytes.len() != 20 { return rejected(); }
    match query(&bytes) {
        Ok(v) if v["context"] == *context
            && v["owner"].as_str().and_then(|a| nus_exchange_contract::codec::decode_address(a).ok())
                .is_some_and(|a| a.as_slice() == bytes) => (200, v),
        _ => rejected(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use bech32::ToBase32;
    fn address(bytes: &[u8]) -> String { bech32::encode("nus", bytes.to_base32(), bech32::Variant::Bech32).unwrap() }
    fn req<'a>(headers: &'a [(&'a str, &'a str)], body: &'a [u8]) -> Request<'a> {
        Request { peer: "127.0.0.1".parse().unwrap(), method: "GET", path: "/dev-local/v1/chain/account", headers, body }
    }
    fn projection() -> Value { json!({"context":{"test":true},"owner":STANDARD.encode([0xab;20]),"fresh":true,"gate":"OPEN","durable_ack":false}) }
    #[test]
    fn authenticated_owner_and_headers_preserved() {
        let h = [("Authorization", "Bearer secret"), ("Origin", "http://localhost:5173")];
        let p = projection();
        let out = account_from(req(&h,b""), &p["context"], |r| {
            assert_eq!(r.path,"/dev-local/v1/account"); assert_eq!(r.headers,h);
            assert!(r.peer.is_loopback()); (200,p.clone())
        }, |owner| { assert_eq!(owner, &[0xab;20]); Ok(json!({"context":p["context"],"owner":address(owner),"sequence":"9"})) });
        assert_eq!(out.0,200); assert_eq!(out.1["sequence"],"9");
        assert!(out.1.get("ledger").is_none());
    }
    #[test]
    fn rejected_auth_never_queries_and_preserves_duplicates() {
        let h = [("authorization","x"),("Authorization","y")];
        for status in [401,403,409,503] {
            assert_eq!(account_from(req(&h,b""), &json!({}), |r| {
                assert_eq!(r.headers,h); (status,json!({"code":"REJECTED"}))
            }, |_| panic!("query after failed authentication")).0,status);
        }
    }
    #[test]
    fn bad_projection_and_body_never_query() {
        let p = projection();
        for (field,value) in [("context",json!({})),("fresh",json!(false)),("gate",json!("RECOVERY_REQUIRED")),("durable_ack",json!(true)),("owner",json!("AB".repeat(20))),("owner",json!("ab"))] {
            let mut bad=p.clone(); bad[field]=value;
            assert_eq!(account_from(req(&[],b""),&p["context"], |_|(200,bad), |_|panic!("query")).0,409);
        }
        assert_eq!(account_from(req(&[],br#"{"owner":"other"}"#),&p["context"], |_|panic!("auth"), |_|panic!("query")).0,400);
    }

    #[test]
    fn actual_account_decoder_projection_crosses_http_boundary() {
        use super::super::collect::{self, account};
        use nus_exchange_contract::s3::{schema, journal::canonical};
        let s = collect::inclusion_tests::snapshot();
        let row = &s.value()["accounts"][0];
        let owner = schema::bytes(&row["owner"]).unwrap();
        let raw = account::tests::rpc(&s, &account::tests::base(&owner,
            &schema::bytes(&row["public_key"]).unwrap(),
            schema::num(&row["account_number"]).unwrap(), schema::num(&row["sequence"]).unwrap()));
        let auth = account::decode(&s, &owner, &raw).unwrap();
        let p = json!({"context":s.context(),"owner":row["owner"],"fresh":true,"gate":"OPEN","durable_ack":false});
        let result = account_from(req(&[], b""), s.context(), |_| (200,p), |bytes| {
            assert_eq!(bytes,owner);
            collect::direct_account_from(&s,bytes,&mut ||Ok(1000),
                ||Ok((s.clone(),canonical(s.value()).unwrap())), |_|Ok(auth))
                .map(|(v,_)|v).map_err(|_|())
        });
        assert_eq!(result.0,200);
        assert_eq!(result.1["owner"],address(&owner));
        assert_eq!(result.1["sequence"],row["sequence"]);
        assert!(result.1.get("ledger").is_none());
        for bad in [json!(hex::encode(&owner)),json!(address(&owner)),json!(STANDARD.encode(&owner).trim_end_matches('='))] {
            let p=json!({"context":s.context(),"owner":bad,"fresh":true,"gate":"OPEN","durable_ack":false});
            assert_eq!(account_from(req(&[],b""),s.context(), |_|(200,p), |_|panic!("IO")).0,409);
        }
    }
    #[test]
    fn mismatched_chain_projection_and_io_are_closed() {
        let p=projection();
        for result in [Err(()),Ok(json!({"context":{},"owner":p["owner"]})),Ok(json!({"context":p["context"],"owner":address(&[0xcd;20])})),Ok(json!({"context":p["context"],"owner":address(&[0xab;20]).to_uppercase()}))] {
            assert_eq!(account_from(req(&[],b""),&p["context"], |_|(200,p.clone()), |_|result),rejected());
        }
    }
}

#[cfg(test)]
#[path="../../../exchange/tests/support/dev_fixture.rs"] mod auth_fixture;
#[cfg(test)]
mod real_auth_tests {
    use super::*;
    use super::auth_fixture as f;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use fips204::{ml_dsa_65, traits::{KeyGen, Signer}};
    use nus_exchange_contract::{codec, s3::{dev_local::{Engine, Validated}, journal::canonical, schema, settlement_local::Options}};
    use std::sync::Arc;
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
    fn real_sessions_bind_chain_owner_and_logout_closes_query() {
        for bps in [0,25] {
            let (inputs,initial)=f::initial(bps); let home=f::home(bps);
            let engine=Arc::new(Engine::create(&home,Validated::new(inputs.clone()).unwrap(),&canonical(&initial).unwrap()).unwrap());
            let commit=engine.reader().get().unwrap().commit.clone();
            let anchor=engine.trusted_recovery_history(&commit,None,1).unwrap().latest.snapshot;
            let o=f::observation(&initial);
            let rest=Rest::new(engine.clone(),anchor.clone(),Options {enabled:true,acknowledge_unproven_space:true,bind:"127.0.0.1".parse().unwrap()}).unwrap();
            let tokens=[login(&rest,&o,0),login(&rest,&o,1)];
            for (i,token) in tokens.iter().enumerate() {
                let authorization=format!("Bearer {token}");
                let headers=[("origin",ORIGIN),("authorization",authorization.as_str())];
                let request=||Request {peer:"127.0.0.1".parse().unwrap(),method:"GET",path:"/dev-local/v1/chain/account",headers:&headers,body:b""};
                let mut queries=0;
                let out=account_from(request(),anchor.context(),|r|rest.handle(r,&o,f::NOW),|owner| {
                    queries+=1; assert_eq!(owner,schema::bytes(&json!(f::owner(i))).unwrap());
                    use bech32::ToBase32;
                    Ok(json!({"context":anchor.context(),"owner":bech32::encode("nus",owner.to_base32(),bech32::Variant::Bech32).unwrap()}))
                });
                assert_eq!(out.0,200,"valid authenticated chain account"); assert_eq!(queries,1);
                let duplicate=[headers[0],headers[1],("Authorization",authorization.as_str())];
                assert_ne!(account_from(Request {headers:&duplicate,..request()},anchor.context(),|r|rest.handle(r,&o,f::NOW),|_|panic!("ambiguous query")).0,200);
                assert_ne!(account_from(Request {peer:"192.0.2.1".parse().unwrap(),..request()},anchor.context(),|r|rest.handle(r,&o,f::NOW),|_|panic!("remote query")).0,200);
                let wrong_origin=[("origin","http://evil.invalid"),headers[1]];
                assert_ne!(account_from(Request {headers:&wrong_origin,..request()},anchor.context(),|r|rest.handle(r,&o,f::NOW),|_|panic!("origin query")).0,200);
                assert_eq!(call(&rest,&o,"/dev-local/v1/auth/logout",json!({}),Some(token)).0,200);
                assert_eq!(account_from(request(),anchor.context(),|r|rest.handle(r,&o,f::NOW),|_|panic!("logged out query")).0,401);
            }
            // Advance the authentication clock only after same-time session tests.
            // Auth deliberately invalidates sessions when wall time moves backward.
            let token=login(&rest,&o,0); let authorization=format!("Bearer {token}");
            let headers=[("origin",ORIGIN),("authorization",authorization.as_str())];
            let request=Request {peer:"127.0.0.1".parse().unwrap(),method:"GET",path:"/dev-local/v1/chain/account",headers:&headers,body:b""};
            assert_eq!(account_from(request,anchor.context(),|r|rest.handle(r,&o,f::NOW+5001),|_|panic!("stale query")).0,409);
            assert_eq!(engine.reader().get().unwrap().commit,commit);
            drop(rest); drop(engine);
            for _ in 0..2 { let e=Engine::open(&home,Validated::new(inputs.clone()).unwrap()).unwrap(); assert_eq!(e.reader().get().unwrap().commit,commit); drop(e); }
            std::fs::remove_dir_all(home).unwrap();
        }
    }
}

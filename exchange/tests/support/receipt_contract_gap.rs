//! CTO-70-03 regression after approved separate public overlay.
use super::*;
use nus_exchange_contract::s3::{
    dev_local::Engine as DevEngine,
    settlement_local::{Options, Request, Rest},
};
use std::sync::Arc;
const ORIGIN: &str = "http://127.0.0.1:5173";
fn call(rest: &Rest, c: &Candidate, path: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
    let authorization = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("origin", ORIGIN)];
    if let Some(a) = &authorization {
        headers.push(("authorization", a.as_str()));
    }
    rest.handle(
        Request {
            peer: "127.0.0.1".parse().unwrap(),
            method: "POST",
            path,
            headers: &headers,
            body: &canonical(body).unwrap(),
        },
        &observation(c),
        NOW,
    )
}
fn login(rest: &Rest, c: &Candidate, i: usize) -> String {
    let (status, challenge) = call(
        rest,
        c,
        "/dev-local/v1/auth/challenge",
        None,
        &json!({"owner":owner(i),"origin":ORIGIN,"audience":"exchange-api"}),
    );
    assert_eq!(status, 200);
    let signature = sign(
        i,
        &codec::frame(
            "NUS/WALLET_AUTH/V1",
            &schema::bytes(&challenge["wire_base64"]).unwrap(),
        ),
    );
    let (status, session) = call(
        rest,
        c,
        "/dev-local/v1/auth/session",
        None,
        &json!({"wire_base64":challenge["wire_base64"],"signature_base64":STANDARD.encode(signature)}),
    );
    assert_eq!(status, 200);
    session["token"].as_str().unwrap().into()
}

#[test]
fn matching_receipt_preserves_original_and_returns_approved_account_projection() {
    for bps in [0, 25] {
        let c = order(setup(bps, false), 0, "2", 2000, 10000, 247);
        let (raw, sig) = sign_order(&c, 1, "1", 1000, 12000, 248);
        let body = json!({"context":c.snapshot().context(),"wire_base64":STANDARD.encode(&raw),"signature_base64":STANDARD.encode(&sig)});
        TRACE.with_borrow_mut(|trace| {
            let t=trace.as_mut().unwrap();let mut e=Arc::new(t.dev.engine.take().unwrap());
            let mut original=None;let mut projection=None;let mut pinned=None;let mut rows=vec![];
            for replay in 0..=2 {
                if replay>0 {drop(e);e=Arc::new(DevEngine::open(&t.dev.home,t.dev.config.clone()).unwrap());}
                let rest=Rest::new(e.clone(),c.snapshot().clone(),Options{enabled:true,acknowledge_unproven_space:true,bind:"127.0.0.1".parse().unwrap()}).unwrap();
                let token=login(&rest,&c,1);let other=login(&rest,&c,0);
                let (status,public)=call(&rest,&c,"/dev-local/v1/orders",Some(&token),&body);assert_eq!(status,200);
                let stored=e.query_signed("ORDER",&raw,&sig,&owner(1),&observation(&c),NOW).unwrap().unwrap();
                assert!(schema::validate("CommandResult",&stored["command_result"]).is_ok());
                assert!(public.get("command_result").is_none());
                assert_eq!(public["envelope_version"],"s3-dev-local-account/1");
                e.verify_account_receipt(schema::num(&public["source"]["command_seq"]).unwrap(), &owner(1), &canonical(&public).unwrap()).unwrap();
                let ledger=stored["command_result"]["ledger_changes"].as_array().unwrap();
                assert!(ledger.iter().any(|v|v["owner"]==owner(0)));
                assert!(ledger.iter().any(|v|v["owner"]==owner(1)));
                assert_eq!(stored["command_result"]["created_fill_ids"].as_array().unwrap().len(),1);
                assert!(stored["command_result"]["affected_order_hashes"].as_array().unwrap().len()>=2);
                assert!(public["account_result"]["ledger_changes"].as_array().unwrap().iter().all(|v|v["owner"]==owner(1)));
                assert!(!public.to_string().contains(&owner(0)));
                assert_eq!(call(&rest,&c,"/dev-local/v1/receipts/orders",Some(&token),&body),(200,public.clone()));
                assert_ne!(call(&rest,&c,"/dev-local/v1/receipts/orders",Some(&other),&body).0,200);
                let view=e.reader().get().unwrap();
                if let Some(before)=&pinned {assert_eq!(&view.commit,before);assert_eq!(original.as_ref(),Some(&stored));assert_eq!(projection.as_ref(),Some(&public));}
                else {pinned=Some(view.commit.clone());original=Some(stored.clone());projection=Some(public.clone());}
                rows.push(json!({"replay":replay,"sequence":view.commit.command_seq,"stored":stored,"public":public,"stored_schema":"PASS","public_schema":"PASS_ACCOUNT_OVERLAY","foreign_owner_in_original":true,"foreign_owner_in_public":false,"other_session_query_denied":true}));
                drop(rest);
            }
            dev_fixture::evidence(&format!("receipt-contract-gap-fee{bps}"),&json!({"characterization":"PASS","product":"PASS_COMPONENT_CTO_70_03","fee_bps":bps,"rows":rows,"same_request_extra_effect":0,"replays":2}));
            dev_fixture::copy_home(&format!("receipt-contract-gap-fee{bps}"),&t.dev.home);
        });
    }
}

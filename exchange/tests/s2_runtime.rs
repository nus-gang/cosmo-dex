mod support;
use base64::{Engine, engine::general_purpose::STANDARD};
use nus_exchange_contract::{
    codec::Codec,
    s2::{
        journal::canonical,
        runtime::{Manifest, Runtime, read_message},
    },
};
use serde_json::{Value, json};
use std::io::Cursor;
use support::*;
#[test]
fn auth_nonce_origin_domain_ttl_logout_and_restart_are_enforced() {
    let d = Dir::new();
    let s = snapshot(NOW);
    let mut r = runtime(&d, &s);
    let mut bad = challenge("order");
    bad["origin"] = json!("http://foreign:5173");
    assert_eq!(call(&mut r, &bad, NOW)["http_status"], "403");
    let ch = call(&mut r, &challenge("order"), NOW);
    let mut auth = login_messages("order", &ch);
    let mut body: Value = serde_json::from_slice(
        &STANDARD
            .decode(auth["body_base64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    let wire = STANDARD
        .decode(body["wire_base64"].as_str().unwrap())
        .unwrap();
    let decoded = Codec::default().decode("WalletChallengeV1", &wire).unwrap();
    assert_eq!(
        decoded["expiry_time"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            - decoded["issued_at"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap(),
        120
    );
    body["signature_base64"] = json!(STANDARD.encode(sign("order", "NUS/ORDER/V1", &wire)));
    let original = auth.clone();
    auth["body_base64"] = json!(STANDARD.encode(canonical(&body).unwrap()));
    assert_eq!(call(&mut r, &auth, NOW)["http_status"], "401");
    let accepted = call(&mut r, &original, NOW);
    assert_eq!(accepted["http_status"], "200");
    assert_eq!(call(&mut r, &original, NOW)["http_status"], "401");
    let token = accepted["body"]["token"].as_str().unwrap();
    let mut read = request("GET", "/s2/me", Some(token), b"");
    assert_eq!(call(&mut r, &read, NOW)["http_status"], "200");
    read["origin"] = json!("http://localhost:5173");
    let denial = call(&mut r, &read, NOW);
    assert_eq!(denial["http_status"], "401");
    assert_eq!(denial["body"]["stream_seq"], Value::Null);
    let new_token = login(&mut r, "order", NOW);
    assert_eq!(
        call(&mut r, &request("GET", "/s2/me", Some(token), b""), NOW)["http_status"],
        "401"
    );
    assert_eq!(
        call(
            &mut r,
            &request("GET", "/s2/me", Some(&new_token), b""),
            NOW + 299_000
        )["http_status"],
        "200"
    );
    assert_eq!(
        call(
            &mut r,
            &request("GET", "/s2/me", Some(&new_token), b""),
            NOW + 300_000
        )["http_status"],
        "401"
    );
    let ch = call(&mut r, &challenge("order"), NOW + 300_000);
    assert_eq!(
        call(&mut r, &login_messages("order", &ch), NOW + 420_000)["http_status"],
        "401"
    );
    let token = login(&mut r, "order", NOW + 420_000);
    assert_eq!(
        call(
            &mut r,
            &request("POST", "/s2/auth/logout", Some(&token), b""),
            NOW + 420_000
        )["http_status"],
        "200"
    );
    assert_eq!(
        call(
            &mut r,
            &request("GET", "/s2/me", Some(&token), b""),
            NOW + 420_000
        )["http_status"],
        "401"
    );
    let token = login(&mut r, "order", NOW + 420_000);
    assert_eq!(
        call(
            &mut r,
            &request("GET", "/s2/me", Some(&token), b""),
            NOW + 419_000
        )["http_status"],
        "401"
    );
    let token = login(&mut r, "order", NOW + 421_000);
    drop(r);
    let mut r = Runtime::start(
        &Manifest::decode(&canonical(&manifest(&s)).unwrap()).unwrap(),
        &d.0.join("journal"),
        None,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut r,
            &request("GET", "/s2/me", Some(&token), b""),
            NOW + 421_000
        )["http_status"],
        "401"
    );
}
#[test]
fn runtime_receipts_private_views_correction_and_recovery_obey_contract() {
    let d = Dir::new();
    let s = snapshot(NOW);
    let mut r = runtime(&d, &s);
    let seller = login(&mut r, "order", NOW);
    let buyer = login(&mut r, "buyer-order", NOW);
    let sell = command(&s, "order", &seller, &[]);
    assert_eq!(call(&mut r, &sell, NOW)["body"]["code"], "CATCHING_UP");
    assert_eq!(call(&mut r, &observe(&s, NOW), NOW)["body"]["mode"], "OPEN");
    let receipt = call(&mut r, &sell, NOW);
    assert_eq!(receipt["http_status"], "201");
    assert_eq!(receipt["body"]["durability"], "LOCAL_FSYNC");
    let retry = call(&mut r, &sell, NOW);
    assert_eq!(retry["http_status"], "200");
    assert_eq!(retry["body"], receipt["body"]);
    assert_eq!(
        call(
            &mut r,
            &command(&s, "order", &seller, &[("max_qty_lots", json!("1999"))]),
            NOW
        )["http_status"],
        "409"
    );
    assert_eq!(
        call(&mut r, &command(&s, "order", &buyer, &[]), NOW)["http_status"],
        "403"
    );
    let bought = call(&mut r, &command(&s, "buyer-order", &buyer, &[]), NOW);
    assert_eq!(bought["http_status"], "201");
    let cancelled = call(&mut r, &command(&s, "cancel", &seller, &[]), NOW);
    assert_eq!(cancelled["http_status"], "201");
    let view = call(&mut r, &request("GET", "/s2/me", Some(&seller), b""), NOW);
    assert_eq!(view["body"]["orders"][0]["state"], "CANCELLED_OFFCHAIN");
    assert_eq!(view["body"]["ledger"][0]["R"], "0");
    assert_eq!(view["body"]["ledger"][0]["D"], "1000000");
    assert_eq!(view["body"]["ledger"][1]["P"], "10000000");
    let path = format!("/s2/me/orders/{}?epoch=0", "33".repeat(32));
    assert_eq!(
        call(&mut r, &request("GET", &path, Some(&buyer), b""), NOW)["body"]["code"],
        "NOT_FOUND_AT_SEQ"
    );
    assert_eq!(
        call(
            &mut r,
            &request("GET", "/s2/me?owner=x", Some(&seller), b""),
            NOW
        )["http_status"],
        "403"
    );
    assert_eq!(
        call(
            &mut r,
            &request("GET", "/s2/me?cursor=bad", Some(&seller), b""),
            NOW
        )["http_status"],
        "409"
    );
    let prepare = request(
        "POST",
        "/s2/me/withdraw-prepare",
        Some(&seller),
        &canonical(&json!({"request_id":"ab".repeat(32)})).unwrap(),
    );
    let held = call(&mut r, &prepare, NOW);
    assert_eq!(held["http_status"], "409");
    assert_eq!(held["body"]["code"], "UNSETTLED_HOLD");
    let network = call(&mut r, &request("GET", "/s2/network", None, b""), NOW);
    let book = call(&mut r, &request("GET", "/s2/book", None, b""), NOW);
    assert!(!book.to_string().contains(&owner("order")));
    let mut next = s.clone();
    next["body"]["observed_height"] = json!("101");
    for a in next["body"]["accounts"].as_array_mut().unwrap() {
        if a["owner"] == owner("order") {
            a["owner_epoch"] = json!("1");
        }
    }
    rehash(&mut next);
    assert_eq!(
        call(&mut r, &observe(&next, NOW), NOW)["http_status"],
        "200"
    );
    let corrected = call(&mut r, &request("GET", &path, Some(&seller), b""), NOW);
    assert_eq!(corrected["body"]["receipt"], receipt["body"]);
    assert_eq!(corrected["body"]["order"]["state"], "CORRECTED");
    let hash = r.service().state().state_hash("OPEN").unwrap();
    let outbox = r.service().state().state_json("OPEN").unwrap()["fills"].clone();
    assert_eq!(outbox[0]["state"], "CORRECTED");
    assert_eq!(outbox[0]["submission_enabled"], false);
    drop(r);
    let mut r = Runtime::start(
        &Manifest::decode(&canonical(&manifest(&s)).unwrap()).unwrap(),
        &d.0.join("journal"),
        None,
    )
    .unwrap();
    assert_eq!(r.service().state().state_hash("OPEN").unwrap(), hash);
    let seller = login(&mut r, "order", NOW);
    assert_eq!(
        call(&mut r, &command(&s, "order", &seller, &[]), NOW)["body"],
        receipt["body"]
    );
    let stale = call(&mut r, &request("GET", "/s2/me", Some(&seller), b""), NOW);
    assert_eq!(stale["body"]["status"]["mode"], "CATCHING_UP");
    assert_eq!(stale["body"]["revision"], stale["body"]["stream_seq"]);
    if let Ok(path) = std::env::var("S2_RUNTIME_SCHEMA_OUTPUT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&vec![
                json!({"type":"CommandReceipt","value":receipt["body"]}),
                json!({"type":"LedgerView","value":view["body"]}),
                json!({"type":"Network","value":network["body"]}),
                json!({"type":"BookSnapshot","value":book["body"]}),
                json!({"type":"LedgerView","value":stale["body"]}),
            ])
            .unwrap(),
        )
        .unwrap();
    }
}
#[test]
fn malformed_control_and_oversized_or_partial_frames_never_execute() {
    let d = Dir::new();
    let s = snapshot(NOW);
    let mut r = runtime(&d, &s);
    for raw in [
        br#"{"op":"rpc_failed","op":"observe"}"#.as_slice(),
        br#"{"op":"rpc_failed","extra":true}"#,
        b"null",
    ] {
        assert_eq!(r.handle(raw, NOW)["body"]["code"], "NON_CANONICAL_WIRE");
    }
    assert_eq!(r.service().state().sequence(), 0);
    assert!(read_message(&mut Cursor::new(vec![b'x'; 100_002])).is_err());
    assert!(read_message(&mut Cursor::new(b"{}".to_vec())).is_err());
    assert_eq!(
        read_message(&mut Cursor::new(b"{}\n".to_vec())).unwrap(),
        Some(b"{}".to_vec())
    );
    let mut wrong = manifest(&s);
    wrong["context"]["config_hash"] = json!("00".repeat(32));
    assert!(Manifest::decode(&canonical(&wrong).unwrap()).is_err());
}

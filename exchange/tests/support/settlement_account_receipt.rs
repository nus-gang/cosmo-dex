//! NUS-71 consumer regressions for the approved account receipt contract.
//! These tests keep public bytes out of the trusted worker/reconciliation lane.
use super::*;
use nus_exchange_contract::s3::settlement_local::{Options, Request, Rest};
use std::sync::Arc;

const ORIGIN: &str = "http://127.0.0.1:5173";

fn commit_value(commit: &nus_exchange_contract::s3::journal::Commit) -> Value {
    json!({
        "command_seq":commit.command_seq.to_string(),
        "record_hash":commit.record_hash,
        "end_offset":commit.end_offset.to_string()
    })
}

fn request(
    rest: &Rest,
    c: &Candidate,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> (u16, Value) {
    request_at(rest, c, method, path, token, body, NOW)
}

fn request_at(
    rest: &Rest,
    c: &Candidate,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
    now: u64,
) -> (u16, Value) {
    let authorization = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("origin", ORIGIN)];
    if let Some(value) = &authorization {
        headers.push(("authorization", value));
    }
    let bytes = body.map(canonical).transpose().unwrap().unwrap_or_default();
    rest.handle(
        Request {
            peer: "127.0.0.1".parse().unwrap(),
            method,
            path,
            headers: &headers,
            body: &bytes,
        },
        &observation(c),
        now,
    )
}

fn login(rest: &Rest, c: &Candidate, i: usize) -> String {
    let (status, challenge) = request(
        rest,
        c,
        "POST",
        "/dev-local/v1/auth/challenge",
        None,
        Some(&json!({"owner":owner(i),"origin":ORIGIN,"audience":"exchange-api"})),
    );
    assert_eq!(status, 200);
    let signature = sign(
        i,
        &codec::frame(
            "NUS/WALLET_AUTH/V1",
            &schema::bytes(&challenge["wire_base64"]).unwrap(),
        ),
    );
    let (status, session) = request(
        rest,
        c,
        "POST",
        "/dev-local/v1/auth/session",
        None,
        Some(
            &json!({"wire_base64":challenge["wire_base64"],"signature_base64":STANDARD.encode(signature)}),
        ),
    );
    assert_eq!(status, 200);
    session["token"].as_str().unwrap().to_owned()
}

#[test]
fn settlement_pins_public_contract_and_keeps_trusted_result_separate() {
    for bps in [0, 25] {
        let c = setup(bps, true);
        TRACE.with_borrow_mut(|trace| {
            let t = trace.as_mut().unwrap();
            let engine = Arc::new(t.dev.engine.take().unwrap());
            let rest = Rest::new(
                engine.clone(),
                c.snapshot().clone(),
                Options {
                    enabled: true,
                    acknowledge_unproven_space: true,
                    bind: "127.0.0.1".parse().unwrap(),
                },
            )
            .unwrap();

            // Authentication precedes path parsing/existence disclosure.
            assert_eq!(
                request(
                    &rest,
                    &c,
                    "GET",
                    "/dev-local/v1/receipts/commands/not-a-seq",
                    None,
                    None,
                ),
                (401, json!({"code":"UNAUTHORIZED","durable_ack":false}))
            );
            let noncanonical_get = json!({});
            assert_eq!(
                request(
                    &rest,
                    &c,
                    "GET",
                    "/dev-local/v1/receipts/commands/not-a-seq",
                    None,
                    Some(&noncanonical_get),
                ),
                (401, json!({"code":"UNAUTHORIZED","durable_ack":false}))
            );
            assert_eq!(
                request(
                    &rest,
                    &c,
                    "GET",
                    "/dev-local/v1/receipts/commands/not-a-seq",
                    Some("invalid-token"),
                    Some(&noncanonical_get),
                ),
                (401, json!({"code":"UNAUTHORIZED","durable_ack":false}))
            );
            let token0 = login(&rest, &c, 0);
            let token1 = login(&rest, &c, 1);
            assert_eq!(
                request(
                    &rest,
                    &c,
                    "GET",
                    "/dev-local/v1/receipts/commands/not-a-seq",
                    Some(&token0),
                    Some(&noncanonical_get),
                ),
                (409, json!({"code":"NON_CANONICAL_WIRE","durable_ack":false}))
            );
            let capabilities = request(
                &rest,
                &c,
                "GET",
                "/dev-local/v1/capabilities",
                Some(&token0),
                None,
            );
            assert_eq!(capabilities.0, 200);
            assert_eq!(capabilities.1["envelope_version"], "s3-dev-local/1");
            assert_eq!(
                capabilities.1["public_receipt_version"],
                "s3-dev-local-account/1"
            );
            assert_eq!(
                capabilities.1["public_receipt_schema_sha256"],
                "2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b"
            );
            assert_eq!(capabilities.1["trusted_receipt_version"], "s3-dev-local/1");

            let (wire, signature) = sign_order(&c, 0, "2", 1000, 10000, 251);
            let body = json!({"context":c.snapshot().context(),"wire_base64":STANDARD.encode(&wire),"signature_base64":STANDARD.encode(&signature)});
            let created = request(
                &rest,
                &c,
                "POST",
                "/dev-local/v1/orders",
                Some(&token0),
                Some(&body),
            );
            assert_eq!(created.0, 200, "{}", created.1);
            let seq = created.1["source"]["command_seq"]
                .as_str()
                .unwrap()
                .to_owned();
            assert_eq!(created.1.as_object().unwrap().len(), 9);
            assert!(created.1.get("command_result").is_none());

            let trusted = engine
                .query_signed(
                    "ORDER",
                    &wire,
                    &signature,
                    &owner(0),
                    &observation(&c),
                    NOW,
                )
                .unwrap()
                .unwrap();
            assert_eq!(trusted["envelope_version"], "s3-dev-local/1");
            assert!(trusted.get("command_result").is_some());
            assert_ne!(trusted, created.1);

            let path = format!("/dev-local/v1/receipts/commands/{seq}");
            assert_eq!(
                request(&rest, &c, "GET", &path, Some(&token0), None),
                created
            );
            assert_eq!(
                request(&rest, &c, "GET", &path, Some(&token1), None),
                (404, json!({"code":"RECEIPT_NOT_FOUND","durable_ack":false}))
            );
            assert_eq!(
                request(
                    &rest,
                    &c,
                    "GET",
                    "/dev-local/v1/receipts/commands/01",
                    Some(&token0),
                    None,
                ),
                (409, json!({"code":"NON_CANONICAL_WIRE","durable_ack":false}))
            );
            assert_eq!(
                request(
                    &rest,
                    &c,
                    "GET",
                    "/dev-local/v1/receipts/commands/999999",
                    Some(&token0),
                    None,
                ),
                (404, json!({"code":"RECEIPT_NOT_FOUND","durable_ack":false}))
            );

            let auth = format!("Bearer {token0}");
            let headers = [("origin", ORIGIN), ("authorization", auth.as_str())];
            let (status, bytes) = rest.handle_bytes(
                Request {
                    peer: "127.0.0.1".parse().unwrap(),
                    method: "GET",
                    path: &path,
                    headers: &headers,
                    body: &[],
                },
                &observation(&c),
                NOW,
            );
            assert_eq!(status, 200);
            assert_eq!(bytes, canonical(&created.1).unwrap());
            engine
                .verify_account_receipt(
                    schema::num(&created.1["source"]["command_seq"]).unwrap(),
                    &owner(0),
                    &bytes,
                )
                .unwrap();
            // Run the monotonic-clock expiry probe last: Auth intentionally
            // invalidates all sessions when time moves backwards afterward.
            assert_eq!(
                request_at(
                    &rest,
                    &c,
                    "GET",
                    "/dev-local/v1/receipts/commands/not-a-seq",
                    Some(&token0),
                    Some(&noncanonical_get),
                    NOW + 301_000,
                ),
                (401, json!({"code":"UNAUTHORIZED","durable_ack":false}))
            );

            let view = engine.reader().get().unwrap();
            dev_fixture::evidence(
                &format!("settlement-account-contract-fee{bps}"),
                &json!({
                    "result":"PASS",
                    "fee_bps":bps,
                    "capabilities":capabilities.1,
                    "public_receipt":created.1,
                    "trusted_receipt":trusted,
                    "canonical_public_base64":STANDARD.encode(bytes),
                    "commit_after_queries":commit_value(&view.commit),
                    "expected_diff":[],
                    "checks":["session_first_401_empty_body","session_first_401_noncanonical_body_missing_invalid_expired","authenticated_noncanonical_body_409","public_schema_pin","trusted_public_type_split","own_query_200","other_query_404","canonical_path_409","missing_404","source_tuple_verification","exact_response_bytes"]
                }),
            );
            dev_fixture::copy_home(
                &format!("settlement-account-contract-fee{bps}"),
                &t.dev.home,
            );
        });
    }
}

#[test]
fn settlement_source_damage_is_503_without_public_fallback() {
    let c = order(setup(0, true), 0, "2", 1000, 10000, 252);
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap();
        let engine = Arc::new(t.dev.engine.take().unwrap());
        let rest = Rest::new(
            engine.clone(),
            c.snapshot().clone(),
            Options {
                enabled: true,
                acknowledge_unproven_space: true,
                bind: "127.0.0.1".parse().unwrap(),
            },
        )
        .unwrap();
        let token = login(&rest, &c, 0);
        let baseline = engine.reader().get().unwrap();
        let journal = t.dev.home.join("journal.dev.wal");
        let mut damaged = std::fs::read(&journal).unwrap();
        let last = damaged.last_mut().unwrap();
        *last ^= 1;
        std::fs::write(&journal, damaged).unwrap();

        let response = request(
            &rest,
            &c,
            "GET",
            "/dev-local/v1/receipts/commands/1",
            Some(&token),
            None,
        );
        assert_eq!(
            response,
            (503, json!({"code":"RECOVERY_REQUIRED","durable_ack":false}))
        );
        let after = engine.reader().get().unwrap();
        assert_eq!(after.commit, baseline.commit);
        assert_eq!(after.state, baseline.state);
        assert_eq!(after.receipts, baseline.receipts);
        dev_fixture::evidence(
            "settlement-account-source-damage",
            &json!({
                "result":"PASS",
                "response":{"status":response.0,"body":response.1},
                "commit_before":commit_value(&baseline.commit),
                "commit_after":commit_value(&after.commit),
                "state_diff":[],
                "receipt_diff":[],
                "public_fallback":false,
                "expected_diff":[]
            }),
        );
    });
}

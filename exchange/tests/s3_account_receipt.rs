#![cfg(feature = "dev-local-settlement")]
#[path = "support/dev_fixture.rs"]
mod fixture;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};
use nus_exchange_contract::{
    codec,
    s3::{
        dev_local::{Engine, Validated},
        journal::{canonical, sha256},
        schema,
        settlement_local::{Options, Request, Rest},
        snapshot::Binding,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};
const NOW: u64 = fixture::NOW;
const ORIGIN: &str = "http://127.0.0.1:5173";
fn key(i: usize) -> (String, Vec<u8>, ml_dsa_65::PrivateKey) {
    let (pk, sk) = ml_dsa_65::KG::keygen_from_seed(&[180 + i as u8; 32]);
    let pk = pk.into_bytes().to_vec();
    (STANDARD.encode(codec::address(&pk).unwrap()), pk, sk)
}
fn fresh(bps: u32) -> (Validated, Value) {
    let (_, mut v) = fixture::initial(bps);
    for (i, a) in v["accounts"].as_array_mut().unwrap().iter_mut().enumerate() {
        let (owner, pk, _) = key(i);
        a["owner"] = json!(owner);
        a["public_key"] = json!(STANDARD.encode(pk));
    }
    v["accounts"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|a| schema::bytes(&a["owner"]).unwrap());
    let input = fixture::inputs(bps, v["accounts"].as_array().unwrap());
    let config = Validated::new(input.clone()).unwrap();
    v["context"] = config.context().clone();
    fixture::finish(&mut v);
    fixture::evidence(&format!("account-input-fee{bps}"), &fixture::bundle(&input));
    (config, v)
}
fn rest(e: Arc<Engine>, config: &Validated, v: &Value) -> Rest {
    Rest::new(
        e,
        Binding::new(
            config.context().clone(),
            v["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| schema::bytes(&a["owner"]).unwrap())
                .collect(),
            [
                codec::integer(&v["assets"][0]["supply_atoms"], 128).unwrap(),
                codec::integer(&v["assets"][1]["supply_atoms"], 128).unwrap(),
            ],
            if v["context"]["config_hash"]
                == sha256(include_bytes!(
                    "../../proposals/s3-local-dev-v1/effective-profile-fee25.json"
                ))
            {
                25
            } else {
                0
            },
        )
        .unwrap()
        .decode(&canonical(v).unwrap())
        .unwrap(),
        Options {
            enabled: true,
            acknowledge_unproven_space: true,
            bind: "127.0.0.1".parse().unwrap(),
        },
    )
    .unwrap()
}
fn call(
    r: &Rest,
    v: &Value,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &Value,
    now: u64,
) -> (u16, Vec<u8>) {
    let auth = token.map(|s| format!("Bearer {s}"));
    let mut headers = vec![("origin", ORIGIN)];
    if let Some(a) = &auth {
        headers.push(("authorization", a.as_str()));
    }
    let raw = if method == "GET" {
        vec![]
    } else {
        canonical(body).unwrap()
    };
    r.handle_bytes(
        Request {
            peer: "127.0.0.1".parse().unwrap(),
            method,
            path,
            headers: &headers,
            body: &raw,
        },
        &fixture::observation(v),
        now,
    )
}
fn value(r: &(u16, Vec<u8>)) -> Value {
    serde_json::from_slice(&r.1).unwrap()
}
fn login(r: &Rest, v: &Value, i: usize) -> String {
    let (owner, _, sk) = key(i);
    let challenge = call(
        r,
        v,
        "POST",
        "/dev-local/v1/auth/challenge",
        None,
        &json!({"owner":owner,"origin":ORIGIN,"audience":"exchange-api"}),
        NOW,
    );
    assert_eq!(
        challenge.0,
        200,
        "{}",
        String::from_utf8_lossy(&challenge.1)
    );
    let challenge = value(&challenge);
    let signature = sk
        .try_sign_with_seed(
            &[51; 32],
            &codec::frame(
                "NUS/WALLET_AUTH/V1",
                &schema::bytes(&challenge["wire_base64"]).unwrap(),
            ),
            &[],
        )
        .unwrap();
    let response = call(
        r,
        v,
        "POST",
        "/dev-local/v1/auth/session",
        None,
        &json!({"wire_base64":challenge["wire_base64"],"signature_base64":STANDARD.encode(signature)}),
        NOW,
    );
    assert_eq!(response.0, 200);
    value(&response)["token"].as_str().unwrap().into()
}
fn order(v: &Value, i: usize, side: &str, q: u64, id: u8) -> Value {
    let f: Value =
        serde_json::from_str(include_str!("../../protocol/s3/vectors/signed.json")).unwrap();
    let mut w = codec::Codec::default()
        .decode(
            "OrderV1",
            &hex::decode(f["cases"][0]["canonical_hex"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
    let (owner, pk, sk) = key(i);
    w["owner"] = json!(owner);
    w["owner_pubkey"] = json!(STANDARD.encode(pk));
    w["genesis_hash"] = v["context"]["genesis_hash"].clone();
    w["side"] = json!(side);
    w["max_qty_lots"] = json!(q.to_string());
    w["limit_price_ticks"] = json!("10000");
    w["order_id"] = json!(hex::encode([id; 32]));
    w["max_fee_bps"] = json!("25");
    let raw = codec::Codec::default().encode("OrderV1", &w).unwrap();
    let sig = sk
        .try_sign_with_seed(&[51; 32], &codec::frame("NUS/ORDER/V1", &raw), &[])
        .unwrap();
    json!({"context":v["context"],"wire_base64":STANDARD.encode(raw),"signature_base64":STANDARD.encode(sig)})
}
fn disk(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, p: &Path, out: &mut BTreeMap<String, String>) {
        for e in std::fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out)
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    sha256(&std::fs::read(p).unwrap()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
#[test]
fn account_fresh_keys_matching_policies_auth_and_two_restarts() {
    for bps in [0, 25] {
        for repetition in 0..3 {
            let (config, v) = fresh(bps);
            let home = fixture::home(bps);
            let mut e =
                Arc::new(Engine::create(&home, config.clone(), &canonical(&v).unwrap()).unwrap());
            let r = rest(e.clone(), &config, &v);
            let tokens: Vec<_> = (0..4).map(|i| login(&r, &v, i)).collect();
            let capability = call(
                &r,
                &v,
                "GET",
                "/dev-local/v1/capabilities",
                Some(&tokens[0]),
                &Value::Null,
                NOW,
            );
            assert_eq!(
                value(&capability)["public_receipt_schema_sha256"],
                nus_exchange_contract::s3::dev_local::PUBLIC_RECEIPT_SCHEMA_SHA256
            );
            let mut receipts = vec![];
            let mut bodies = vec![];
            // Distinct makers: partial fill and multiple makers in one taker result.
            for (i, side, q, id) in [(0, "2", 500, 81), (2, "2", 2000, 82), (1, "1", 1000, 83)] {
                let body = order(&v, i, side, q, id);
                let response = call(
                    &r,
                    &v,
                    "POST",
                    "/dev-local/v1/orders",
                    Some(&tokens[i]),
                    &body,
                    NOW,
                );
                assert_eq!(response.0, 200, "{}", String::from_utf8_lossy(&response.1));
                let public = value(&response);
                assert_eq!(public["account_result"]["code"], "OK");
                assert!(public.get("command_result").is_none());
                assert_eq!(public.as_object().unwrap().len(), 9);
                let seq = schema::num(&public["source"]["command_seq"]).unwrap();
                e.verify_account_receipt(seq, &key(i).0, &response.1)
                    .unwrap();
                let src = e.trusted_receipt_source(seq).unwrap().unwrap();
                assert_eq!(sha256(src.frame()), public["source"]["record_hash"]);
                for j in 0..4 {
                    if j != i {
                        assert!(!String::from_utf8_lossy(&response.1).contains(&key(j).0));
                    }
                }
                fixture::evidence(
                    &format!("account-source-fee{bps}-r{repetition}-s{seq}"),
                    &json!({"principal":key(i).0,"context":v["context"],"public_base64":STANDARD.encode(&response.1),"frame_base64":STANDARD.encode(src.frame()),"trusted_result":src.result()}),
                );
                receipts.push((i, seq, response));
                bodies.push(body);
            }
            assert_eq!(
                value(&receipts[2].2)["account_result"]["created_fill_ids"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            let local = json!({"context":v["context"],"request_id":"91".repeat(32)});
            let prepare = call(
                &r,
                &v,
                "POST",
                "/dev-local/v1/withdraw/prepare",
                Some(&tokens[2]),
                &local,
                NOW,
            );
            assert_eq!(prepare.0, 200);
            assert_eq!(value(&prepare)["account_result"]["code"], "UNSETTLED_HOLD");
            assert_eq!(value(&prepare)["account_result"]["state"], "REJECTED");
            assert!(
                !value(&prepare)["account_result"]["affected_order_hashes"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            let commit = e.reader().get().unwrap().commit.clone();
            assert_eq!(
                call(
                    &r,
                    &v,
                    "POST",
                    "/dev-local/v1/withdraw/prepare",
                    Some(&tokens[2]),
                    &local,
                    NOW
                ),
                prepare
            );
            assert_eq!(e.reader().get().unwrap().commit, commit);
            let abort = call(
                &r,
                &v,
                "POST",
                "/dev-local/v1/withdraw/abort",
                Some(&tokens[2]),
                &json!({"context":v["context"],"request_id":"92".repeat(32)}),
                NOW,
            );
            assert_eq!(abort.0, 200);
            // Uniform seq not-found and strict path/auth priority.
            let absent = json!({"code":"RECEIPT_NOT_FOUND","durable_ack":false});
            assert_eq!(
                value(&call(
                    &r,
                    &v,
                    "GET",
                    "/dev-local/v1/receipts/commands/1",
                    Some(&tokens[1]),
                    &Value::Null,
                    NOW
                )),
                absent
            );
            assert_eq!(
                value(&call(
                    &r,
                    &v,
                    "GET",
                    "/dev-local/v1/receipts/commands/999",
                    Some(&tokens[1]),
                    &Value::Null,
                    NOW
                )),
                absent
            );
            for p in [
                "0",
                "01",
                "+1",
                "1?principal=x",
                "1/x",
                "18446744073709551616",
            ] {
                let route = format!("/dev-local/v1/receipts/commands/{p}");
                assert_eq!(call(&r, &v, "GET", &route, None, &Value::Null, NOW).0, 401);
                assert_eq!(
                    call(&r, &v, "GET", &route, Some(&tokens[0]), &Value::Null, NOW).0,
                    409
                );
            }
            assert_eq!(
                call(
                    &r,
                    &v,
                    "POST",
                    "/dev-local/v1/receipts/orders",
                    Some(&tokens[0]),
                    &bodies[2],
                    NOW
                )
                .0,
                403
            );
            let mut bad = bodies[2].clone();
            bad["signature_base64"] = json!(STANDARD.encode([0; 3309]));
            assert_ne!(
                call(
                    &r,
                    &v,
                    "POST",
                    "/dev-local/v1/receipts/orders",
                    Some(&tokens[1]),
                    &bad,
                    NOW
                )
                .0,
                200
            );
            assert_eq!(
                call(
                    &r,
                    &v,
                    "GET",
                    "/dev-local/v1/receipts/commands/1",
                    Some(&tokens[0]),
                    &Value::Null,
                    NOW + 301000
                )
                .0,
                401
            );
            let baseline = e.reader().get().unwrap();
            let files = disk(&home);
            drop(r);
            let mut replays = vec![];
            for replay in 0..=2 {
                if replay > 0 {
                    drop(e);
                    e = Arc::new(Engine::open(&home, config.clone()).unwrap());
                }
                let r = rest(e.clone(), &config, &v);
                let ts: Vec<_> = (0..4).map(|i| login(&r, &v, i)).collect();
                for ((i, seq, expected), body) in receipts.iter().zip(&bodies) {
                    for route in ["/dev-local/v1/orders", "/dev-local/v1/receipts/orders"] {
                        assert_eq!(
                            call(&r, &v, "POST", route, Some(&ts[*i]), body, NOW + 6001),
                            *expected
                        );
                    }
                    assert_eq!(
                        call(
                            &r,
                            &v,
                            "GET",
                            &format!("/dev-local/v1/receipts/commands/{seq}"),
                            Some(&ts[*i]),
                            &Value::Null,
                            NOW + 6001
                        ),
                        *expected
                    );
                    let mut changed = expected.1.clone();
                    changed[10] ^= 1;
                    assert!(
                        e.verify_account_receipt(*seq, &key(*i).0, &changed)
                            .is_err()
                    );
                }
                let actual = e.reader().get().unwrap();
                assert_eq!(actual.commit, baseline.commit);
                assert_eq!(actual.state, baseline.state);
                assert_eq!(actual.receipts, baseline.receipts);
                assert_eq!(disk(&home), files);
                replays.push(json!({"replay":replay,"store_diff":[],"state_diff":[],"result_diff":[],"public_bytes_diff":[]}));
            }
            fixture::evidence(
                &format!("account-live-fee{bps}-r{repetition}"),
                &json!({"scope":"COMPONENT_FRESH_MOCK_KEYS_NO_SOCKET","result":"PASS","context":v["context"],"receipt_ledger":receipts.iter().map(|(i,s,r)|json!({"principal":key(*i).0,"seq":s.to_string(),"raw_base64":STANDARD.encode(&r.1)})).collect::<Vec<_>>(),"prepare":value(&prepare),"abort":value(&abort),"replays":replays,"trusted_ledger":baseline.receipts,"state":baseline.state,"files":files}),
            );
            fixture::copy_home(&format!("account-live-fee{bps}-r{repetition}"), &home);
        }
    }
}
#[test]
fn account_source_corruption_closes_queries_and_mutations() {
    for bps in [0, 25] {
        for mode in ["wal", "marker", "raw", "pair_deleted", "semantic"] {
            let (config, v) = fresh(bps);
            let home = fixture::home(bps);
            let e =
                Arc::new(Engine::create(&home, config.clone(), &canonical(&v).unwrap()).unwrap());
            let r = rest(e.clone(), &config, &v);
            let token = login(&r, &v, 0);
            let body = order(&v, 0, "2", 500, 90);
            assert_eq!(
                call(
                    &r,
                    &v,
                    "POST",
                    "/dev-local/v1/orders",
                    Some(&token),
                    &body,
                    NOW
                )
                .0,
                200
            );
            let before = e.reader().get().unwrap();
            let wal = home.join("journal.dev.wal");
            let mut bytes = std::fs::read(&wal).unwrap();
            let mut record: Value = serde_json::from_slice(&bytes[72..]).unwrap();
            match mode {
                "wal" => {
                    let n = bytes.len();
                    bytes[n - 5] ^= 1;
                    std::fs::write(&wal, bytes).unwrap();
                }
                "marker" => {
                    let mut marker: Value = serde_json::from_slice(
                        &std::fs::read(home.join("commit.dev.json")).unwrap(),
                    )
                    .unwrap();
                    marker["record_hash"] = json!(schema::ZERO);
                    std::fs::write(home.join("commit.dev.json"), canonical(&marker).unwrap())
                        .unwrap();
                }
                "raw" | "pair_deleted" => {
                    let p = home.join("bootstrap.dev.json");
                    if mode == "raw" {
                        let mut raw = std::fs::read(&p).unwrap();
                        raw[0] ^= 1;
                        std::fs::write(p, raw).unwrap();
                    } else {
                        std::fs::remove_file(p).unwrap();
                        std::fs::remove_file(home.join("home.dev.json")).unwrap();
                    }
                }
                _ => {
                    record["signature_hash"] = json!(schema::ZERO);
                    let payload = canonical(&record).unwrap();
                    let mut b = b"S3D1".to_vec();
                    b.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                    b.extend(hex::decode(sha256(&payload)).unwrap());
                    b.extend(hex::decode(sha256(&b)).unwrap());
                    b.extend(payload);
                    std::fs::write(&wal, &b).unwrap();
                    let marker = json!({"command_seq":"1","record_hash":sha256(&b),"end_offset":b.len().to_string()});
                    std::fs::write(home.join("commit.dev.json"), canonical(&marker).unwrap())
                        .unwrap();
                }
            }
            let damaged = disk(&home);
            let query = call(
                &r,
                &v,
                "GET",
                "/dev-local/v1/receipts/commands/1",
                Some(&token),
                &Value::Null,
                NOW,
            );
            assert_eq!(query.0, 503, "{mode}");
            assert_eq!(
                call(
                    &r,
                    &v,
                    "POST",
                    "/dev-local/v1/orders",
                    Some(&token),
                    &body,
                    NOW
                )
                .0,
                503
            );
            assert_eq!(e.reader().get().unwrap().commit, before.commit);
            assert_eq!(e.reader().get().unwrap().state, before.state);
            assert_eq!(disk(&home), damaged);
            drop(r);
            drop(e);
            for _ in 0..2 {
                assert!(Engine::open(&home, config.clone()).is_err(), "{mode}");
                assert_eq!(disk(&home), damaged);
            }
            fixture::evidence(
                &format!("account-corrupt-fee{bps}-{mode}"),
                &json!({"result":"PASS","query":value(&query),"state_diff":[],"files_after_fault":damaged,"automatic_repair":0,"open_rejections":2}),
            );
        }
    }
}

#[test]
#[cfg(feature = "fault-injection")]
fn account_marker_failures_never_return_public_success_or_repair() {
    use nus_exchange_contract::s3::dev_local::Error;
    for bps in [0, 25] {
        for point in [
            "candidate_verified",
            "marker_sync",
            "after_marker_dir_sync",
            "before_publish",
            "before_response",
        ] {
            let (config, v) = fresh(bps);
            let home = fixture::home(bps);
            let e =
                Arc::new(Engine::create(&home, config.clone(), &canonical(&v).unwrap()).unwrap());
            let r = rest(e.clone(), &config, &v);
            let token = login(&r, &v, 0);
            let body = order(&v, 0, "2", 500, 93);
            e.set_fault_hook(Some(Arc::new(move |p| {
                if p == point {
                    Err(Error::Recovery("RECEIPT_IO_PROBE"))
                } else {
                    Ok(())
                }
            })))
            .unwrap();
            let response = call(
                &r,
                &v,
                "POST",
                "/dev-local/v1/orders",
                Some(&token),
                &body,
                NOW,
            );
            assert_eq!(response.0, 503);
            assert_eq!(
                call(
                    &r,
                    &v,
                    "GET",
                    "/dev-local/v1/receipts/commands/1",
                    Some(&token),
                    &Value::Null,
                    NOW
                )
                .0,
                503
            );
            let bytes = disk(&home);
            drop(r);
            drop(e);
            let mut results = vec![];
            for replay in 1..=2 {
                match Engine::open(&home, config.clone()) {
                    Ok(e) => {
                        assert!(["before_publish", "before_response"].contains(&point));
                        let public = e.account_receipt(1, &key(0).0).unwrap().unwrap();
                        let source = e.trusted_receipt_source(1).unwrap().unwrap();
                        results.push(json!({"replay":replay,"public_base64":STANDARD.encode(public.as_bytes()),"frame_base64":STANDARD.encode(source.frame()),"principal":key(0).0,"context":v["context"]}));
                    }
                    Err(_) => {
                        assert!(!["before_publish", "before_response"].contains(&point));
                        results.push(json!({"replay":replay,"error":"RECOVERY_REQUIRED"}));
                    }
                }
                assert_eq!(disk(&home), bytes);
            }
            fixture::evidence(
                &format!("account-marker-fee{bps}-{point}"),
                &json!({"result":"PASS","initial_response":value(&response),"rounds":results,"automatic_repair":0,"same_home_diff":[]}),
            );
            fixture::copy_home(&format!("account-marker-fee{bps}-{point}"), &home);
        }
    }
}

#[test]
fn account_overlay_rejects_old_or_resealed_inputs_without_home_migration() {
    let (_, v) = fixture::initial(0);
    for tamper in [false, true] {
        let mut inputs = fixture::inputs(0, v["accounts"].as_array().unwrap());
        if tamper {
            inputs
                .files
                .get_mut("proposals/s3-local-account-receipt-v1/schema.json")
                .unwrap()
                .push(b' ');
        } else {
            inputs
                .files
                .retain(|p, _| !p.starts_with("proposals/s3-local-account-receipt-v1/"));
        }
        let mut manifest: Value = serde_json::from_slice(&inputs.runtime_manifest).unwrap();
        manifest["files_sha256"] = json!(
            inputs
                .files
                .iter()
                .map(|(p, b)| (p.clone(), sha256(b)))
                .collect::<BTreeMap<_, _>>()
        );
        manifest["contract_sha256"] = json!(fixture::aggregate(&inputs.files));
        inputs.runtime_manifest = canonical(&manifest).unwrap();
        inputs.approved_runtime_sha256 = sha256(&inputs.runtime_manifest);
        // A caller-controlled pin cannot replace the build's approved overlay.
        assert!(Validated::new(inputs).is_err());
    }
}

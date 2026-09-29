use base64::{Engine, engine::general_purpose::STANDARD};
use nus_exchange_contract::{
    codec::{Codec, address, frame},
    decision::*,
    policy::*,
};
use serde_json::{Value, json};
fn vectors() -> Value {
    serde_json::from_str(include_str!("../../protocol/v1/vectors/decision-port.json")).unwrap()
}
#[test]
fn common_rc3_policy_examples() {
    let v = vectors();
    for c in v["fee_cases"].as_array().unwrap() {
        assert_eq!(
            fee_json(&c["receive"], &c["active_bps"]).unwrap_or_else(str::to_owned),
            c["expected"],
            "{}",
            c["id"]
        );
    }
    for c in v["cap_cases"].as_array().unwrap() {
        assert_eq!(
            cap_check(&c["cap"], &c["active_bps"])
                .map(|_| "OK")
                .unwrap_or_else(|e| e),
            c["expected"],
            "{}",
            c["id"]
        );
    }
    for c in v["decision_cases"].as_array().unwrap() {
        assert_eq!(
            evaluate_snapshot(
                c["input"]["authentication_result"].clone(),
                &c["input"]["snapshot"]
            ),
            c["expected"],
            "{}",
            c["id"]
        );
    }
    for c in v["api_errors"].as_array().unwrap() {
        assert_eq!(
            api_error(c["code"].as_str().unwrap(), None).unwrap(),
            c["expected"]
        );
    }
    println!(
        "PASS rc3 fee=17 cap=21 synthetic decisions=34 API=6; injected authentication is NOT crypto evidence"
    );
}
fn ctx(pk: &[u8]) -> OrderContext<'_> {
    OrderContext {
        snapshot_id: "same-snapshot",
        chain_id: "nus-m0-local",
        genesis_hash: "1111111111111111111111111111111111111111111111111111111111111111",
        exchange_module_id: "x/exchange",
        market_id: "BASE-QUOTE",
        market_config_version: 1,
        registered_key_type: Some("ML-DSA-65"),
        registered_key: Some(pk),
        epoch: 7,
        height: 999,
        revoked: false,
        filled: 0,
        available: 1000000,
        fee_bps: 0,
    }
}
#[test]
fn actual_registration_and_signature_at_top_level() {
    let sigs: Value =
        serde_json::from_str(include_str!("../../protocol/v1/vectors/signatures.json")).unwrap();
    let v = &sigs["positives"][0];
    let raw = hex::decode(v["canonical_hex"].as_str().unwrap()).unwrap();
    let sig = hex::decode(v["signature_hex"].as_str().unwrap()).unwrap();
    let pk = hex::decode(v["public_key_hex"].as_str().unwrap()).unwrap();
    let mut different = pk.clone();
    different[0] ^= 1;
    for c in vectors()["registration_cases"].as_array().unwrap() {
        let reg = &c["registered"];
        let mut context = ctx(&pk);
        context.registered_key = if reg.is_null() {
            None
        } else if reg["raw_key_ref"] == "submitted" {
            Some(&pk)
        } else {
            Some(&different)
        };
        context.registered_key_type = reg["key_type"].as_str();
        let got = validate_order(&raw, &sig, &context)
            .map(|_| "OK")
            .unwrap_or_else(|e| e);
        assert_eq!(got, c["expected_registration"], "{}", c["id"]);
        let out = admit_order(&raw, &sig, &context, &Value::Null);
        assert_eq!(
            out["authentication"],
            stage(authenticate_order(&raw, &sig, &context).map(|_| ()))
        );
        assert_eq!(out["ack"], "NOT_CONNECTED");
    }
    let mut bad = sig;
    bad[0] ^= 1;
    assert_eq!(
        admit_order(&raw, &bad, &ctx(&pk), &Value::Null)["authentication"]["code"],
        "INVALID_SIGNATURE"
    );
    println!("PASS real ML-DSA top-level registration=5 plus tampered signature");
}
#[test]
fn newly_signed_cap_and_small_fill() {
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, SerDes, Signer},
    };
    let (pk, sk) = ml_dsa_65::KG::keygen_from_seed(&[42u8; 32]);
    let pk = pk.into_bytes();
    let sigs: Value =
        serde_json::from_str(include_str!("../../protocol/v1/vectors/signatures.json")).unwrap();
    let raw = hex::decode(sigs["positives"][0]["canonical_hex"].as_str().unwrap()).unwrap();
    let codec = Codec::default();
    let mut o = codec.decode("OrderV1", &raw).unwrap();
    o["owner_pubkey"] = json!(STANDARD.encode(pk));
    o["owner"] = json!(STANDARD.encode(address(&pk).unwrap()));
    for cap in ["0", "25", "10000", "10001", "4294967295"] {
        o["max_fee_bps"] = json!(cap);
        let raw = codec.encode("OrderV1", &o).unwrap();
        let sig = sk
            .try_sign_with_seed(&[43u8; 32], &frame("NUS/ORDER/V1", &raw), &[])
            .unwrap();
        assert!(validate_order(&raw, &sig, &ctx(&pk)).is_ok(), "cap {cap}");
        let mut context = ctx(&pk);
        context.fee_bps = 10001;
        assert_eq!(validate_order(&raw, &sig, &context), Err("BPS_RANGE"));
    }
    o["max_qty_lots"] = json!("1");
    o["limit_price_ticks"] = json!("1");
    o["max_fee_bps"] = json!("25");
    let raw = codec.encode("OrderV1", &o).unwrap();
    let sig = sk
        .try_sign_with_seed(&[44u8; 32], &frame("NUS/ORDER/V1", &raw), &[])
        .unwrap();
    let s = json!({"id":"same-snapshot","source":"SYNTHETIC","height":"999","expiry_height":"1000",
      "epoch_matches":true,"revoked":false,"id_state":"NEW","cumulative_ok":true,"confirmed_balance_ok":true,
      "q":"1","p":"1","active_bps":"25","cap":"25"});
    let mut context = ctx(&pk);
    context.fee_bps = 25;
    let out = admit_order(&raw, &sig, &context, &s);
    assert_eq!(out["authentication"], json!({"status":"PASS","code":"OK"}));
    assert_eq!(out["snapshot_policy"]["code"], "FEE_GE_RECEIVE");
    assert_eq!(out["ack"], "NOT_CONNECTED");
    assert_eq!(out["wal_replay"], "NOT_RUN");
    assert_eq!(out["ledger"], "NOT_CONNECTED");
    assert_eq!(validate_order(&raw, &sig, &context), Err("FEE_GE_RECEIVE"));
    let mut wrong = s.clone();
    wrong["q"] = json!("2");
    assert_eq!(
        admit_order(&raw, &sig, &context, &wrong)["snapshot_policy"]["status"],
        "NOT_CONNECTED"
    );
    for field in ["height", "q", "active_bps", "confirmed_balance_ok", "id"] {
        let mut missing = s.clone();
        missing[field] = Value::Null;
        assert_eq!(
            admit_order(&raw, &sig, &context, &missing)["snapshot_policy"]["status"],
            "NOT_CONNECTED"
        );
    }
    let mut invalid = s;
    invalid["revoked"] = json!("false");
    assert_eq!(
        admit_order(&raw, &sig, &context, &invalid)["snapshot_policy"]["status"],
        "NOT_CONNECTED"
    );
    assert!(api_error("NOT_CONNECTED", None).is_none());
    println!(
        "PASS re-signed cap=5, active range=5, small-fill auth PASS/policy rejection; missing/mismatched snapshot never ACKED"
    );
}

#[test]
fn json_lines_port_uses_real_registration() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let sigs: Value =
        serde_json::from_str(include_str!("../../protocol/v1/vectors/signatures.json")).unwrap();
    let v = &sigs["positives"][0];
    let mut requests = vec![json!({"op":"fee","receive":"0","active_bps":"0"})];
    for reg in [
        json!({"key_type":"ML-DSA-65","raw_key_hex":v["public_key_hex"]}),
        json!({"key_type":"OTHER","raw_key_hex":v["public_key_hex"]}),
        Value::Null,
        json!({"raw_key_hex":v["public_key_hex"]}),
    ] {
        requests.push(json!({"op":"admit_order","wire_hex":v["canonical_hex"],"signature_hex":v["signature_hex"],
        "context":{"snapshot_id":"s1","height":"999","epoch":"7","chain_id":"nus-m0-local",
        "genesis_hash":"1111111111111111111111111111111111111111111111111111111111111111",
        "exchange_module_id":"x/exchange","market_id":"BASE-QUOTE","market_config_version":"1","registered":reg},
        "snapshot":null}));
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_decision"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let rows: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["result"], "0");
    for (row, expected) in rows[1..].iter().zip([
        stage(Ok(())),
        stage(Err("ACCOUNT_KEY_MISMATCH")),
        stage(Err("ACCOUNT_KEY_UNREGISTERED")),
        stage(Err("NOT_CONNECTED")),
    ]) {
        assert_eq!(row["authentication"], expected);
        assert_eq!(row["ack"], "NOT_CONNECTED");
    }
}

#[test]
fn signed_enum_market_and_epoch_at_admission_boundary() {
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, SerDes, Signer},
    };
    let (pk, sk) = ml_dsa_65::KG::keygen_from_seed(&[42u8; 32]);
    let pk = pk.into_bytes();
    let sigs: Value =
        serde_json::from_str(include_str!("../../protocol/v1/vectors/signatures.json")).unwrap();
    let codec = Codec::default();
    let mut base = codec
        .decode(
            "OrderV1",
            &hex::decode(sigs["positives"][0]["canonical_hex"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
    base["owner_pubkey"] = json!(STANDARD.encode(pk));
    base["owner"] = json!(STANDARD.encode(address(&pk).unwrap()));
    base["max_qty_lots"] = json!("100");
    base["limit_price_ticks"] = json!("100");
    base["max_fee_bps"] = json!("25");
    let mut requests = Vec::new();
    let mut cases = Vec::new();
    for side in ["1", "2", "3"] {
        for kind in ["1", "2", "3"] {
            cases.push((
                format!("enum-{side}-{kind}"),
                json!({"side":side,"order_type":kind}),
                7,
                true,
                if side == "3" || kind == "3" {
                    "MARKET_LIMIT"
                } else {
                    "OK"
                },
            ));
        }
    }
    for (field, value, expected) in [
        ("max_qty_lots", "0", "MARKET_LIMIT"),
        ("max_qty_lots", "1000000", "OK"),
        ("max_qty_lots", "1000001", "MARKET_LIMIT"),
        ("limit_price_ticks", "0", "MARKET_LIMIT"),
        ("limit_price_ticks", "1000000", "OK"),
        ("limit_price_ticks", "1000001", "MARKET_LIMIT"),
        ("fee_asset_policy_id", "OTHER", "MARKET_LIMIT"),
    ] {
        cases.push((
            format!("market-{field}-{value}"),
            json!({field:value}),
            7,
            true,
            expected,
        ));
    }
    for (epoch, flag, expected) in [
        (7, true, "OK"),
        (999, false, "EPOCH_MISMATCH"),
        (7, false, "NOT_CONNECTED"),
        (999, true, "NOT_CONNECTED"),
    ] {
        cases.push((
            format!("epoch-{epoch}-{flag}"),
            json!({}),
            epoch,
            flag,
            expected,
        ));
    }
    for (id, updates, epoch, flag, expected) in cases {
        let mut order = base.clone();
        for (k, v) in updates.as_object().unwrap() {
            order[k] = v.clone();
        }
        let raw = codec.encode("OrderV1", &order).unwrap();
        let sig = sk
            .try_sign_with_seed(&[43u8; 32], &frame("NUS/ORDER/V1", &raw), &[])
            .unwrap();
        let snapshot = json!({"id":"same-snapshot","source":"SYNTHETIC","height":"999","expiry_height":order["expiry_height"],
            "epoch_matches":flag,"revoked":false,"id_state":"NEW","cumulative_ok":true,"confirmed_balance_ok":true,
            "q":order["max_qty_lots"],"p":order["limit_price_ticks"],"active_bps":"25","cap":order["max_fee_bps"]});
        let mut context = ctx(&pk);
        context.epoch = epoch;
        let out = admit_order(&raw, &sig, &context, &snapshot);
        assert_eq!(out["authentication"], stage(Ok(())), "{id}");
        let want = stage(if expected == "OK" {
            Ok(())
        } else {
            Err(expected)
        });
        assert_eq!(out["snapshot_policy"]["status"], want["status"], "{id}");
        assert_eq!(out["snapshot_policy"]["code"], want["code"], "{id}");
        assert_eq!(out["ack"], "NOT_CONNECTED");
        assert_eq!(out["ledger"], "NOT_CONNECTED");
        assert_eq!(out["wal_replay"], "NOT_RUN");
        requests.push(json!({"id":id,"expected":expected,"op":"admit_order","wire_hex":hex::encode(&raw),"signature_hex":hex::encode(sig),
            "context":{"snapshot_id":"same-snapshot","height":"999","epoch":epoch.to_string(),"chain_id":"nus-m0-local",
            "genesis_hash":context.genesis_hash,"exchange_module_id":"x/exchange","market_id":"BASE-QUOTE","market_config_version":"1",
            "registered":{"key_type":"ML-DSA-65","raw_key_hex":hex::encode(pk)}},"snapshot":snapshot}));
    }
    // Exercise the real CLI too, including omitted epoch: never silently default to zero.
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut missing = requests[0].clone();
    missing["context"].as_object_mut().unwrap().remove("epoch");
    missing["id"] = json!("missing-epoch");
    requests.push(missing);
    let mut child = Command::new(env!("CARGO_BIN_EXE_decision"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    // Avoid pipe deadlock: request payloads exceed a pipe buffer; output is small.
    for r in &requests {
        writeln!(input, "{r}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let rows: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), requests.len());
    for (r, out) in requests.iter().zip(&rows) {
        if r["id"] == "missing-epoch" {
            // rc4 preserves authentication and observed ID when context is incomplete.
            assert_eq!(out["authentication"]["status"], "PASS");
            assert_eq!(out["snapshot_policy"]["status"], "NOT_CONNECTED");
            assert_eq!(out["snapshot_policy"]["snapshot_id"], "same-snapshot");
            continue;
        }
        assert_eq!(out["authentication"]["status"], "PASS");
        let e = r["expected"].as_str().unwrap();
        let want = json!({"status": if e == "OK" {"PASS"} else if e == "NOT_CONNECTED" {"NOT_CONNECTED"} else {"REJECTED"}, "code": if e == "NOT_CONNECTED" {Value::Null} else {json!(e)}});
        assert_eq!(
            out["snapshot_policy"]["status"], want["status"],
            "{}",
            r["id"]
        );
        assert_eq!(out["snapshot_policy"]["code"], want["code"], "{}", r["id"]);
    }
    if let Ok(dir) = std::env::var("NUS_ADMISSION_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/inputs.json"),
            serde_json::to_vec_pretty(&requests).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/rust-results.json"),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    println!(
        "PASS admission: 9 signed enum combinations, 7 market boundaries, 4 epoch combinations, 21 CLI requests"
    );
}

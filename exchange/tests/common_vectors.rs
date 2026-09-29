use base64::{Engine, engine::general_purpose::STANDARD};
use nus_exchange_contract::{adapter::Reservation, codec::*, policy::*};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../protocol/v1")
}
fn vector(n: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(root().join("vectors").join(n)).unwrap()).unwrap()
}
fn h(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap()).unwrap()
}
fn n(v: &Value) -> u64 {
    integer(v, 64).unwrap() as u64
}
fn arr(v: &Value) -> &Vec<Value> {
    v.as_array().unwrap()
}
fn fixture_api(name: &str, fields: &Value) -> Value {
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(root().join("schema.json")).unwrap()).unwrap();
    let mut o = serde_json::Map::new();
    for f in arr(&schema[name]) {
        let tag = f["tag"].as_u64().unwrap();
        let input = arr(fields)
            .iter()
            .find(|v| v[0].as_u64() == Some(tag))
            .unwrap();
        let v = match f["type"].as_str().unwrap() {
            "a" | "pk" | "sig" => json!(STANDARD.encode(h(&input[2]))),
            _ => input[2].clone(),
        };
        o.insert(f["name"].as_str().unwrap().into(), v);
    }
    Value::Object(o)
}
#[test]
fn real_signature_vectors() {
    let d = vector("signatures.json");
    let codec = Codec::default();
    for v in arr(&d["positives"]) {
        let name = match v["id"].as_str().unwrap() {
            "order" => "OrderV1",
            "cancel" => "CancelV1",
            "wallet" => "WalletChallengeV1",
            _ => panic!(),
        };
        let api = fixture_api(name, &v["fields"]);
        let raw = codec.encode(name, &api).unwrap();
        assert_eq!(raw, h(&v["canonical_hex"]));
        assert_eq!(codec.decode(name, &raw).unwrap(), api);
        let domain = String::from_utf8(h(&v["domain_hex"])).unwrap();
        let framed = frame(&domain, &raw);
        assert_eq!(framed, h(&v["sign_input_hex"]));
        assert_eq!(hex::encode(hash(&framed)), v["sha256"]);
        let pk = h(&v["public_key_hex"]);
        assert_eq!(address(&pk).unwrap().to_vec(), h(&v["owner_raw_hex"]));
        assert!(
            verify_raw(&pk, &framed, &h(&v["signature_hex"]), &[]),
            "{}",
            v["id"]
        );
    }
    for v in arr(&d["negatives"]) {
        assert!(
            !verify_raw(
                &h(&v["public_key_hex"]),
                &h(&v["message_hex"]),
                &h(&v["signature_hex"]),
                &h(&v["context_hex"])
            ),
            "{}",
            v["id"]
        )
    }
    println!("PASS ML-DSA-65 positives=3 negatives=35, independent API→wire→frame→SHA256");
}
#[test]
fn strict_wire_and_amount_vectors() {
    let codec = Codec::default();
    let mut count = 0;
    for file in ["wire-cases.json", "message-codec.json"] {
        let d = vector(file);
        let cases = if file == "wire-cases.json" {
            &d["cases"]
        } else {
            &d["wire_cases"]
        };
        for v in arr(cases) {
            let result = codec.decode(v["message"].as_str().unwrap(), &h(&v["wire_hex"]));
            assert_eq!(
                result.is_ok(),
                v["expected"] == "CANONICAL",
                "{}: {:?}",
                v["id"],
                result
            );
            count += 1;
        }
    }
    for v in arr(&vector("amount-codec.json")["cases"]) {
        let good = if let Some(api) = v.get("api_json") {
            integer(api, 128)
                .map(|n| {
                    assert_eq!(hex::encode(n.to_be_bytes()), v["wire_hex"]);
                    assert_eq!(STANDARD.encode(n.to_be_bytes()), v["wire_base64"]);
                    assert_eq!(n.to_string(), api.as_str().unwrap());
                })
                .is_ok()
        } else {
            h(&v["wire_hex"]).len() == 16
        };
        assert_eq!(good, v["expected"] == "OK", "{v}")
    }
    println!("PASS wire={count} amount_codec=17");
}
#[test]
fn message_vectors() {
    let codec = Codec::default();
    let d = vector("message-codec.json");
    for v in arr(&d["positives"]) {
        let name = v["message"].as_str().unwrap();
        let raw = codec.encode(name, &v["api_json"]).unwrap();
        assert_eq!(raw, h(&v["canonical_hex"]), "{}", v["id"]);
        assert_eq!(codec.decode(name, &raw).unwrap(), v["api_json"]);
        if name == "TransferStableV1" {
            let framed = frame("NUS/PAYMENT_ID/V1", &raw);
            assert_eq!(framed, h(&v["payment_frame_hex"]));
            assert_eq!(hex::encode(hash(&framed)), v["payment_hash"]);
        }
    }
    println!("PASS complete_message_codec={}", arr(&d["positives"]).len());
}
#[test]
fn integer_vectors() {
    let text = fs::read_to_string(root().join("vectors/integers.tsv")).unwrap();
    for l in text.lines() {
        let c = l.split('\t').collect::<Vec<_>>();
        let parse = |s: &str| -> Result<u128, &str> {
            if s.is_empty()
                || (s.len() > 1 && s.starts_with('0'))
                || !s.bytes().all(|b| b.is_ascii_digit())
            {
                return Err("FORMAT");
            }
            s.parse().map_err(|_| "RANGE")
        };
        let result = (|| {
            let a = parse(c[2])?;
            let b = parse(c[3])?;
            let z = parse(c[4])?;
            match c[1] {
                "mul" => checked_product(a, b, z),
                "add" => a.checked_add(b).ok_or("FINAL_U128_OVERFLOW"),
                "sub" => a.checked_sub(b).ok_or("UNDERFLOW"),
                "fee" => fee(a, u32::try_from(b).map_err(|_| "BPS_RANGE")?),
                _ => panic!(),
            }
        })();
        let actual = match result {
            Ok(n) => n.to_string(),
            Err(e) => e.into(),
        };
        assert_eq!(actual, c[5], "{}", c[0]);
    }
    println!("PASS integer_boundaries=32");
}
#[test]
fn common_policy() {
    let d = vector("policy-cases.json");
    for c in arr(&d["cases"]) {
        let good = match c["kind"].as_str().unwrap() {
            "order_and_cancel_expiry" => expiry(n(&c["height"]), n(&c["expiry_height"])).is_ok(),
            "wallet_policy" => wallet_policy(
                n(&c["issued_at"]),
                n(&c["expiry_time"]),
                n(&c["now"]),
                c["origin"].as_str().unwrap(),
                c["allowed_origin"].as_str().unwrap(),
                c["audience"].as_str().unwrap(),
                c["nonce_consumed"].as_bool().unwrap(),
            ),
            _ => panic!(),
        };
        assert_eq!(good, c["expected"] == "allow", "{}", c["id"]);
    }
    println!("PASS policy={}", arr(&d["cases"]).len());
}
#[test]
fn s0_exchange_vectors() {
    let d = vector("s0-cases.json");
    let mut count = 0;
    for c in arr(&d["cases"]) {
        let i = &c["input"];
        let actual = match c["kind"].as_str().unwrap() {
            "expiry" => json!(
                expiry(n(&i["height"]), n(&i["expiry_height"]))
                    .map(|_| "OK")
                    .unwrap_or_else(|e| e)
            ),
            "atoms" => json!(
                integer(&i["value"], 128)
                    .map(|_| "OK")
                    .unwrap_or_else(|e| e)
            ),
            "fill" => match fill(n(&i["q"]), n(&i["p"]), n(&i["bps"]) as u32) {
                Ok((b, q, fb, fq)) => {
                    json!({"base":b.to_string(),"quote":q.to_string(),"fee_base":fb.to_string(),"fee_quote":fq.to_string()})
                }
                Err(e) => json!(e),
            },
            _ => continue,
        };
        assert_eq!(actual, c["expected"], "{}", c["id"]);
        count += 1;
    }
    println!("PASS S0 exchange cases={count}; receipt/transfer state belongs to Settlement/Chain");
}
#[test]
fn order_validation_precedence_and_binding() {
    let d = vector("signatures.json");
    let v = &d["positives"][0];
    let raw = h(&v["canonical_hex"]);
    let sig = h(&v["signature_hex"]);
    let pk = h(&v["public_key_hex"]);
    let mut ctx = OrderContext {
        chain_id: "nus-m0-local",
        genesis_hash: "1111111111111111111111111111111111111111111111111111111111111111",
        exchange_module_id: "x/exchange",
        market_id: "BASE-QUOTE",
        market_config_version: 1,
        registered_key: Some(&pk),
        epoch: 7,
        height: 999,
        revoked: false,
        filled: 0,
        available: 200,
        fee_bps: 0,
    };
    assert!(validate_order(&raw, &sig, &ctx).is_ok());
    ctx.height = 1000;
    assert_eq!(validate_order(&raw, &sig, &ctx), Err("EXPIRED"));
    ctx.registered_key = None;
    assert_eq!(
        validate_order(&raw, &sig, &ctx),
        Err("ACCOUNT_KEY_UNREGISTERED")
    );
    ctx.chain_id = "wrong";
    assert_eq!(validate_order(&raw, &sig, &ctx), Err("CONTEXT_MISMATCH"));
    ctx.chain_id = "nus-m0-local";
    ctx.height = 999;
    ctx.registered_key = Some(&pk);
    ctx.available = 199;
    assert_eq!(
        validate_order(&raw, &sig, &ctx),
        Err("INSUFFICIENT_CONFIRMED_BALANCE")
    );
    ctx.available = 200;
    ctx.filled = 3;
    assert_eq!(
        validate_order(&raw, &sig, &ctx),
        Err("CUMULATIVE_QTY_EXCEEDED")
    );
    ctx.filled = 0;
    let mut bad = sig.clone();
    bad[0] ^= 1;
    assert_eq!(validate_order(&raw, &bad, &ctx), Err("INVALID_SIGNATURE"));
    let mut api = Codec::default().decode("OrderV1", &raw).unwrap();
    api["owner"] = json!(STANDARD.encode([0u8; 20]));
    let wrong = Codec::default().encode("OrderV1", &api).unwrap();
    assert_eq!(validate_order(&wrong, &sig, &ctx), Err("ADDRESS_MISMATCH"));
}
#[test]
fn parser_adversarial_boundaries() {
    let c = Codec::default();
    assert!(c.encode_json("OrderV1", r#"{"x":1,"x":2}"#).is_err());
    assert_eq!(c.decode("OrderV1", &vec![0; 8193]), Err("RESOURCE_LIMIT"));
    for b in [32, 64, 128] {
        let max = if b == 128 {
            u128::MAX
        } else {
            (1u128 << b) - 1
        };
        assert_eq!(integer(&json!(max.to_string()), b), Ok(max));
        if b < 128 {
            assert!(integer(&json!((max + 1).to_string()), b).is_err());
        }
    }
    assert_eq!(
        cumulative(u64::MAX, 1, u64::MAX),
        Err("CUMULATIVE_QTY_EXCEEDED")
    );
    assert_eq!(cumulative(1, 1, 2), Ok(2));
    assert_eq!(cumulative(2, 1, 2), Err("CUMULATIVE_QTY_EXCEEDED"));
    use bech32::{ToBase32, Variant};
    let s = bech32::encode("nus", [1u8; 20].to_base32(), Variant::Bech32).unwrap();
    assert_eq!(decode_address(&s), Ok([1; 20]));
    assert!(decode_address(&s.to_uppercase()).is_err());
    let wrong = bech32::encode("cosmos", [1u8; 20].to_base32(), Variant::Bech32).unwrap();
    assert!(decode_address(&wrong).is_err());
}
#[test]
fn sequenced_ioc_contract() {
    let mut r = Reservation::buy(10, 5, 100).unwrap();
    r.trade(10, 0, 3, 90).unwrap();
    assert_eq!((r.remaining, r.debit, r.pending_receive), (200, 300, 3000));
    let before = r.clone();
    assert_eq!(r.trade(10, 1, 3, 90), Err("CUMULATIVE_QTY_EXCEEDED"));
    assert_eq!(r, before);
    r.trade(10, 0, 3, 90).unwrap();
    assert_eq!(r, before);
    assert_eq!(r.trade(10, 0, 2, 90), Err("ID_CONFLICT"));
    assert_eq!(r.complete_ioc(9), Err("COMMAND_SEQUENCE_MISMATCH"));
    assert_eq!(r.complete_ioc(10), Ok(200));
    assert_eq!(r.complete_ioc(10), Ok(0));
    assert_eq!((r.debit, r.pending_receive), (300, 3000));
    r.trade(10, 0, 3, 90).unwrap();
    assert_eq!(r.trade(10, 1, 1, 90), Err("CALLBACK_AFTER_COMPLETE"));
}
#[test]
fn cli_runner_receipt() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vectors"))
        .arg(root().join("vectors/signatures.json"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["contract_revision"], "1.0.0-rc2");
    assert_eq!(receipt["results"].as_array().unwrap().len(), 38);
}

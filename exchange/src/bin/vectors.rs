//! SRE argv lane: cargo run --locked --bin vectors -- /absolute/signatures.json
use base64::{Engine, engine::general_purpose::STANDARD};
use nus_exchange_contract::codec::{Codec, address, frame, hash, verify_raw};
use serde_json::{Value, json};
fn h(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().expect("hex string")).expect("valid hex")
}
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: vectors PATH_TO_signatures.json");
    let bytes = std::fs::read(path).expect("read common vector");
    let digest = hex::encode(hash(&bytes));
    assert_eq!(
        digest, "4f55d2806f98ece56c4a7a7f148373ecaa82e6360c121310c124ed29db4f8217",
        "unexpected vector baseline"
    );
    let d: Value = serde_json::from_slice(&bytes).unwrap();
    let schema: Value =
        serde_json::from_str(include_str!("../../../protocol/v1/schema.json")).unwrap();
    let mut results = vec![];
    let codec = Codec::default();
    let positives = d["positives"].as_array().unwrap();
    let negatives = d["negatives"].as_array().unwrap();
    assert_eq!(positives.len(), 3);
    assert_eq!(negatives.len(), 35);
    for (v, name) in positives
        .iter()
        .zip(["OrderV1", "CancelV1", "WalletChallengeV1"])
    {
        let mut api = serde_json::Map::new();
        for f in schema[name].as_array().unwrap() {
            let entry = v["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e[0] == f["tag"])
                .unwrap();
            let value = match f["type"].as_str().unwrap() {
                "a" | "pk" | "sig" => json!(STANDARD.encode(h(&entry[2]))),
                _ => entry[2].clone(),
            };
            api.insert(f["name"].as_str().unwrap().into(), value);
        }
        let body = codec.encode(name, &Value::Object(api)).unwrap();
        assert_eq!(body, h(&v["canonical_hex"]));
        let domain = match name {
            "OrderV1" => "NUS/ORDER/V1",
            "CancelV1" => "NUS/CANCEL/V1",
            _ => "NUS/WALLET_AUTH/V1",
        };
        let message = frame(domain, &body);
        assert_eq!(message, h(&v["sign_input_hex"]));
        assert_eq!(hex::encode(hash(&message)), v["sha256"]);
        let pk = h(&v["public_key_hex"]);
        assert_eq!(address(&pk).unwrap().to_vec(), h(&v["owner_raw_hex"]));
        let valid = verify_raw(&pk, &message, &h(&v["signature_hex"]), &[]);
        assert!(valid);
        results.push(json!({"id":v["id"],"sign_bytes_hex":hex::encode(message),"valid":valid}));
    }
    for v in negatives {
        let message = h(&v["message_hex"]);
        let valid = verify_raw(
            &h(&v["public_key_hex"]),
            &message,
            &h(&v["signature_hex"]),
            &h(&v["context_hex"]),
        );
        assert!(!valid, "{}", v["id"]);
        results.push(json!({"id":v["id"],"sign_bytes_hex":hex::encode(message),"valid":valid}));
    }
    println!(
        "{}",
        json!({"contract_revision":"1.0.0-rc3","vectors_sha256":digest,"results":results})
    );
}

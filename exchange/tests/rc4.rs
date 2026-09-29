use base64::{Engine, engine::general_purpose::STANDARD};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};
use nus_exchange_contract::codec::{Codec, address, frame};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};
#[test]
fn rc4_full_outputs_through_actual_signature_and_cli() {
    let vectors: Value = serde_json::from_str(include_str!(
        "../../protocol/v1/vectors/snapshot-output.json"
    ))
    .unwrap();
    let signatures: Value =
        serde_json::from_str(include_str!("../../protocol/v1/vectors/signatures.json")).unwrap();
    let (pk, sk) = ml_dsa_65::KG::keygen_from_seed(&[42; 32]);
    let pk = pk.into_bytes();
    let codec = Codec::default();
    let mut base = codec
        .decode(
            "OrderV1",
            &hex::decode(
                signatures["positives"][0]["canonical_hex"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    base["owner_pubkey"] = json!(STANDARD.encode(pk));
    base["owner"] = json!(STANDARD.encode(address(&pk).unwrap()));
    let mut evidence = Vec::new();
    for case in vectors["cases"].as_array().unwrap() {
        let input = &case["input"];
        let mut order = base.clone();
        for (k, v) in input["authenticated_order"].as_object().unwrap() {
            order[k] = v.clone();
        }
        let wire = codec.encode("OrderV1", &order).unwrap();
        let mut sig = sk
            .try_sign_with_seed(&[43; 32], &frame("NUS/ORDER/V1", &wire), &[])
            .unwrap();
        if input["authentication_result"]["status"] == "REJECTED" {
            sig[0] ^= 1;
        }
        let mut context = input["context"].clone();
        for k in [
            "chain_id",
            "genesis_hash",
            "exchange_module_id",
            "market_id",
            "market_config_version",
        ] {
            context[k] = order[k].clone();
        }
        context["registered"] = json!({"raw_key_hex":hex::encode(pk),"key_type":"ML-DSA-65"});
        if input["authentication_result"]["status"] == "NOT_CONNECTED" {
            context["registered"]
                .as_object_mut()
                .unwrap()
                .remove("key_type");
        }
        let request = json!({"op":"admit_order","wire_hex":hex::encode(wire),"signature_hex":hex::encode(sig),"context":context,"snapshot":input["snapshot"]});
        let mut child = Command::new(env!("CARGO_BIN_EXE_decision"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual, case["expected"], "{}", case["id"]);
        evidence.push(
            json!({"id":case["id"],"request":request,"expected":case["expected"],"actual":actual}),
        );
    }
    assert_eq!(evidence.len(), 60);
    if let Ok(dir) = std::env::var("NUS_RC4_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/rc4-full-outputs.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
    println!(
        "PASS rc4 60 full outputs; actual ML-DSA signatures, corrupted signature and disconnected registration; ACK/ledger NOT_CONNECTED"
    );
}

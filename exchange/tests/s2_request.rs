use base64::{Engine, engine::general_purpose::STANDARD};
use nus_exchange_contract::s2::request::{self, SignedCommand, SignedKind};
use serde_json::{Value, json};
fn fixture(id: &str) -> (Value, Value) {
    let vectors: Value =
        serde_json::from_str(include_str!("../../protocol/s2/vectors/signed.json")).unwrap();
    let v = vectors["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap();
    let s: Value =
        serde_json::from_str(include_str!("../../protocol/s2/vectors/snapshot.json")).unwrap();
    let context = s["snapshot"]["body"]["context"].clone();
    (
        json!({"context":context,"wire_base64":STANDARD.encode(hex::decode(v["canonical_hex"].as_str().unwrap()).unwrap()),
        "signature_base64":STANDARD.encode(hex::decode(v["signature_hex"].as_str().unwrap()).unwrap())}),
        context,
    )
}
#[test]
fn signed_envelopes_preserve_bytes_and_reject_ambiguous_input() {
    for (id, kind) in [("order", SignedKind::Order), ("cancel", SignedKind::Cancel)] {
        let (value, context) = fixture(id);
        let decode =
            |v: &Value| SignedCommand::decode(kind, &serde_json::to_vec(v).unwrap(), &context);
        let command = decode(&value).unwrap();
        assert_eq!(command.kind, kind);
        assert_eq!(STANDARD.encode(&command.wire), value["wire_base64"]);
        assert_eq!(
            STANDARD.encode(&command.signature),
            value["signature_base64"]
        );
        for key in ["owner", "kind", "seed"] {
            let mut bad = value.clone();
            bad[key] = json!("injected");
            assert!(decode(&bad).is_err());
        }
        for key in ["context", "wire_base64", "signature_base64"] {
            let mut bad = value.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(decode(&bad).is_err());
            let mut bad = value.clone();
            bad[key] = Value::Null;
            assert!(decode(&bad).is_err());
        }
        for invalid in ["", "AA", "AB==", "AA==\n", "____"] {
            let mut bad = value.clone();
            bad["signature_base64"] = json!(invalid);
            assert!(decode(&bad).is_err());
        }
        for key in context.as_object().unwrap().keys() {
            let mut bad = value.clone();
            bad["context"][key] = json!("other");
            assert!(decode(&bad).is_err());
        }
        let raw = serde_json::to_string(&value).unwrap();
        let duplicate = raw.replacen('{', "{\"wire_base64\":\"AA==\",", 1);
        assert!(SignedCommand::decode(kind, duplicate.as_bytes(), &context).is_err());
        let nested = raw.replace(
            "\"schema_version\":\"1\"",
            "\"schema_version\":\"1\",\"schema_version\":\"1\"",
        );
        assert!(SignedCommand::decode(kind, nested.as_bytes(), &context).is_err());
        let padded = format!(
            "{}{}",
            raw,
            " ".repeat(request::MAX_REQUEST_BYTES - raw.len())
        );
        assert!(SignedCommand::decode(kind, padded.as_bytes(), &context).is_ok());
        assert!(SignedCommand::decode(kind, format!("{padded} ").as_bytes(), &context).is_err());
        assert!(
            SignedCommand::decode(
                if kind == SignedKind::Order {
                    SignedKind::Cancel
                } else {
                    SignedKind::Order
                },
                raw.as_bytes(),
                &context
            )
            .is_err()
        );
    }
}
#[test]
fn local_actions_and_origin_do_not_accept_owner_or_origin_aliases() {
    let id = "ab".repeat(32);
    let raw = format!("{{ \"request_id\" : \"{id}\" }}");
    assert_eq!(
        request::local_action(raw.as_bytes()).unwrap(),
        format!("{{\"request_id\":\"{id}\"}}").as_bytes()
    );
    for raw in [
        format!("{{\"request_id\":\"{id}\",\"owner\":\"x\"}}"),
        format!("{{\"request_id\":\"{}\"}}", id.to_uppercase()),
        "{\"request_id\":null}".into(),
        format!("{{\"request_id\":\"{id}\",\"request_id\":\"{id}\"}}"),
        "{}".into(),
    ] {
        assert!(request::local_action(raw.as_bytes()).is_err());
    }
    for origin in [
        None,
        Some("null"),
        Some("http://localhost:5173/"),
        Some("http://localhost:5173.evil"),
        Some("https://localhost:5173"),
        Some("http://LOCALHOST:5173"),
    ] {
        assert!(request::mutation_origin(origin).is_err());
    }
    for origin in ["http://127.0.0.1:5173", "http://localhost:5173"] {
        assert_eq!(request::mutation_origin(Some(origin)).unwrap(), origin);
    }
}

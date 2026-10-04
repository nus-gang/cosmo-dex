#![allow(dead_code)]
use base64::{Engine, engine::general_purpose::STANDARD};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, Signer},
};
use nus_exchange_contract::{
    codec::{self, Codec},
    s2::{
        journal::{canonical, sha256},
        runtime::{Manifest, Runtime},
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
pub const NOW: u64 = 1790956680000;
pub const ORIGIN: &str = "http://127.0.0.1:5173";
static NEXT: AtomicU64 = AtomicU64::new(0);
pub struct Dir(pub PathBuf);
impl Dir {
    pub fn new() -> Self {
        let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!(
            "s2-runtime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn vector(id: &str) -> Value {
    let v: Value =
        serde_json::from_str(include_str!("../../../protocol/s2/vectors/signed.json")).unwrap();
    v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap()
        .clone()
}
pub fn owner(id: &str) -> String {
    STANDARD.encode(hex::decode(vector(id)["owner_raw_hex"].as_str().unwrap()).unwrap())
}
pub fn sign(id: &str, domain: &str, wire: &[u8]) -> Vec<u8> {
    let seed: [u8; 32] = hex::decode(vector(id)["test_seed_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let (_, sk) = ml_dsa_65::KG::keygen_from_seed(&seed);
    sk.try_sign_with_seed(&[77; 32], &codec::frame(domain, wire), &[])
        .unwrap()
        .to_vec()
}
pub fn signed(id: &str, changes: &[(&str, Value)]) -> (Vec<u8>, Vec<u8>) {
    let v = vector(id);
    let raw = hex::decode(v["canonical_hex"].as_str().unwrap()).unwrap();
    let (name, domain) = if id == "cancel" {
        ("CancelV1", "NUS/CANCEL/V1")
    } else {
        ("OrderV1", "NUS/ORDER/V1")
    };
    let mut value = Codec::default().decode(name, &raw).unwrap();
    for (key, v) in changes {
        value[*key] = v.clone();
    }
    let raw = Codec::default().encode(name, &value).unwrap();
    let sig = sign(id, domain, &raw);
    (raw, sig)
}
pub fn snapshot(now: u64) -> Value {
    let mut v =
        serde_json::from_str::<Value>(include_str!("../../../protocol/s2/vectors/snapshot.json"))
            .unwrap()["snapshot"]
            .clone();
    let pin: Value =
        serde_json::from_str(include_str!("../../../protocol/s2/manifest.json")).unwrap();
    v["body"]["context"]["contract_hash"] = pin["contract_sha256"].clone();
    v["body"]["context"]["config_hash"] = pin["config_sha256"].clone();
    v["body"]["block_time_unix_ms"] = json!(now.to_string());
    rehash(&mut v);
    v
}
pub fn rehash(snapshot: &mut Value) {
    snapshot["snapshot_id"] = json!(sha256(&codec::frame(
        "NUS/S2/SNAPSHOT/V1",
        &canonical(&snapshot["body"]).unwrap()
    )));
}
pub fn manifest(snapshot: &Value) -> Value {
    json!({"context":snapshot["body"]["context"],"market":snapshot["body"]["market"],
        "owners":snapshot["body"]["accounts"].as_array().unwrap().iter().map(|a|a["owner"].clone()).collect::<Vec<_>>(),
        "supplies":["2000000000000","2000000000000"],"bootstrap_snapshot_id":snapshot["snapshot_id"]})
}
pub fn runtime(dir: &Dir, snapshot: &Value) -> Runtime {
    Runtime::start(
        &Manifest::decode(&canonical(&manifest(snapshot)).unwrap()).unwrap(),
        &dir.0.join("journal"),
        Some(&canonical(snapshot).unwrap()),
    )
    .unwrap()
}
pub fn request(method: &str, path: &str, token: Option<&str>, body: &[u8]) -> Value {
    json!({"op":"request","method":method,"path":path,"origin":ORIGIN,
        "authorization":token.map(|t|format!("Bearer {t}")),"body_base64":STANDARD.encode(body)})
}
pub fn call(runtime: &mut Runtime, message: &Value, now: u64) -> Value {
    runtime.handle(&canonical(message).unwrap(), now)
}
pub fn login_messages(id: &str, response: &Value) -> Value {
    let wire = STANDARD
        .decode(response["body"]["wire_base64"].as_str().unwrap())
        .unwrap();
    request("POST","/s2/auth/sessions",None,&canonical(&json!({"wire_base64":STANDARD.encode(&wire),"signature_base64":STANDARD.encode(sign(id,"NUS/WALLET_AUTH/V1",&wire))})).unwrap())
}
pub fn challenge(id: &str) -> Value {
    request(
        "POST",
        "/s2/auth/challenges",
        None,
        &canonical(&json!({"owner":owner(id),"origin":ORIGIN,"audience":"exchange-api"})).unwrap(),
    )
}
pub fn login(runtime: &mut Runtime, id: &str, now: u64) -> String {
    let response = call(runtime, &challenge(id), now);
    assert_eq!(response["http_status"], "200", "{response}");
    let response = call(runtime, &login_messages(id, &response), now);
    assert_eq!(response["http_status"], "200", "{response}");
    response["body"]["token"].as_str().unwrap().to_owned()
}
pub fn command(snapshot: &Value, id: &str, token: &str, changes: &[(&str, Value)]) -> Value {
    let (wire, sig) = signed(id, changes);
    request("POST",if id=="cancel" {"/s2/cancels"} else {"/s2/orders"},Some(token),&canonical(&json!({"context":snapshot["body"]["context"],"wire_base64":STANDARD.encode(wire),"signature_base64":STANDARD.encode(sig)})).unwrap())
}
pub fn observe(snapshot: &Value, now: u64) -> Value {
    json!({"op":"observe","snapshot":snapshot,"cursor_height":snapshot["body"]["observed_height"],
        "received_at_unix_ms":now.to_string(),"query_latency_ms":"1","catching_up":false})
}

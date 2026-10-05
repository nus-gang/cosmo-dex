#![allow(dead_code)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nus_exchange_contract::{
    codec,
    s3::{
        dev_local::{Command, Inputs, Validated},
        journal::{canonical, sha256},
        schema,
        snapshot::Observation,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub const NOW: u64 = 1791193000000;
pub fn key(i: usize) -> Value {
    let k: Value =
        serde_json::from_str(include_str!("../../../protocol/s3/vectors/test-keys.json")).unwrap();
    k[i].clone()
}
pub fn owner(i: usize) -> String {
    STANDARD.encode(hex::decode(key(i)["owner_raw_hex"].as_str().unwrap()).unwrap())
}
pub fn aggregate(files: &BTreeMap<String, Vec<u8>>) -> String {
    sha256(
        files
            .iter()
            .map(|(p, b)| format!("{}  {p}\n", sha256(b)))
            .collect::<String>()
            .as_bytes(),
    )
}
pub fn inputs(bps: u32, users: &[Value]) -> Inputs {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = BTreeMap::new();
    let baseline = std::fs::read(root.join("protocol/s3/manifest.json")).unwrap();
    let m: Value = serde_json::from_slice(&baseline).unwrap();
    for (p, h) in m["files_sha256"].as_object().unwrap() {
        let raw = match p.as_str() {
            "exchange/Cargo.toml" => include_bytes!("rc3-exchange-Cargo.toml").to_vec(),
            "chain/app/go.mod" => {
                std::fs::read(root.join("chain/app/testdata/local-demo/rc3-chain-app-go.mod"))
                    .unwrap()
            }
            _ => std::fs::read(root.join(p)).unwrap(),
        };
        assert_eq!(sha256(&raw), h.as_str().unwrap(), "inherited {p}");
        files.insert(p.clone(), raw);
    }
    files.insert("protocol/s3/manifest.json".into(), baseline);
    let prefix = "proposals/s3-local-dev-v1/";
    let raw = std::fs::read(root.join(format!("{prefix}MANIFEST.json"))).unwrap();
    let m: Value = serde_json::from_slice(&raw).unwrap();
    for p in m["files_sha256"].as_object().unwrap().keys() {
        files.insert(
            format!("{prefix}{p}"),
            std::fs::read(root.join(format!("{prefix}{p}"))).unwrap(),
        );
    }
    files.insert(format!("{prefix}MANIFEST.json"), raw);
    let mut components = BTreeMap::new();
    for name in ["chain", "exchange", "settlement", "wallet", "sre"] {
        let p = format!("chain/local-demo/components/{name}.json");
        files.insert(p.clone(),canonical(&json!({"head":"497ecba3008de9168c431facc4ff9fc8a4fc329b","tree":"0024b2f4da7d7157216ba9f03bbb79fd3d94412a","implementation_settings":{"scope":"SYNTHETIC_COMPONENT_TEST_NOT_RUNTIME_APPROVAL"}})).unwrap());
        components.insert(name, p);
    }
    let contract = aggregate(&files);
    let m = json!({"format":"s3-dev-local-runtime/1","scope":"REVIEWED_RUNTIME","candidate_manifest_sha256":"90169d322336a0c0de9bc6c48725d528d42fe74c78ea5b596fc7e059d747dda2","contract_sha256":contract,"files_sha256":files.iter().map(|(p,b)|(p.clone(),sha256(b))).collect::<BTreeMap<_,_>>(),"components":components});
    let runtime_manifest = canonical(&m).unwrap();
    let pin = sha256(&runtime_manifest);
    let profile = files[&format!("{prefix}effective-profile-fee{bps}.json")].clone();
    let pk = |i| STANDARD.encode(hex::decode(key(i)["public_key_hex"].as_str().unwrap()).unwrap());
    let genesis=serde_json::to_vec(&json!({"genesis_time":"2023-11-14T22:13:20Z","chain_id":"nus-s3-dev-1","initial_height":"1","consensus_params":{"block":{"max_bytes":"1048576","max_gas":"20000000"},"evidence":{"max_bytes":"65536"}},"validators":(0..4).map(|i|json!({"pub_key":{"type":"tendermint/PubKeyEd25519","value":STANDARD.encode([i;32])},"power":"10"})).collect::<Vec<_>>(),"app_state":{"public_keys":users.iter().map(|a|a["public_key"].clone()).collect::<Vec<_>>(),"settlement_operator_public_keys":[pk(16),pk(17)],"admin_public_key":pk(18),"fee_bps":bps.to_string(),"contract_hash":contract,"config_hash":sha256(&profile)}})).unwrap();
    let guard=canonical(&json!({"envelope_version":"s3-dev-local/1","profile_id":"s3-dev-local-v1","candidate_manifest_sha256":"90169d322336a0c0de9bc6c48725d528d42fe74c78ea5b596fc7e059d747dda2","runtime_manifest_sha256":pin,"effective_profile_sha256":sha256(&profile),"run_uuid":"00000000-0000-4000-8000-000000000070","fee_profile":format!("fee{bps}"),"context":{"service_schema":"s3/3","chain_id":"nus-s3-dev-1","genesis_hash":sha256(&genesis),"contract_hash":contract,"config_hash":sha256(&profile),"market_id":"DEVBASE/DEVQUOTE","market_config_version":"1"}})).unwrap();
    Inputs {
        approved_runtime_sha256: pin,
        runtime_manifest,
        files,
        effective_profile: profile,
        guard,
        genesis,
        acknowledge_unproven_space: true,
    }
}
pub fn initial(bps: u32) -> (Inputs, Value) {
    let a: Value = serde_json::from_str(include_str!(
        "../../../protocol/s3/vectors/correction-state-hash.json"
    ))
    .unwrap();
    let mut v = a["initial_state"]["chain_snapshot"].clone();
    v["height"] = json!("100");
    v["block_time_unix_ms"] = json!(NOW.to_string());
    v["operator"] = json!(owner(16));
    v["terminal_batch_seqs"] = json!([]);
    v["last_batch_seq"] = json!("0");
    v["last_batch_hash"] = json!(schema::ZERO);
    let input = inputs(bps, v["accounts"].as_array().unwrap());
    let c = Validated::new(input.clone()).unwrap();
    v["context"] = c.context().clone();
    finish(&mut v);
    (input, v)
}
pub fn finish(v: &mut Value) {
    v.as_object_mut().unwrap().remove("snapshot_id");
    v["snapshot_id"] = json!(schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", v).unwrap());
}
static COUNT: AtomicU64 = AtomicU64::new(0);
pub fn home(bps: u32) -> PathBuf {
    let base = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .or_else(|| std::env::var_os("NUS_TEST_TMPDIR"))
        .expect("set NUS_TEST_TMPDIR or PAPERCLIP_RUN_SCRATCH_DIR");
    let path = std::fs::canonicalize(base)
        .unwrap()
        .join(format!(
            "nus70-test-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ))
        .join(".runtime/s3-dev-local-v1/00000000-0000-4000-8000-000000000070")
        .join(format!("fee{bps}"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
}
pub fn observation(v: &Value) -> Observation {
    Observation {
        snapshot_id: v["snapshot_id"].as_str().unwrap().into(),
        cursor_height: schema::num(&v["height"]).unwrap(),
        received_at: NOW,
        query_latency_ms: 1,
        catching_up: false,
    }
}
pub fn sign_order(v: &Value, i: usize, side: &str, q: u64, p: u64, id: u8) -> (Vec<u8>, Vec<u8>) {
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, Signer},
    };
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../protocol/s3/vectors/signed.json")).unwrap();
    let mut wire = codec::Codec::default()
        .decode(
            "OrderV1",
            &hex::decode(fixtures["cases"][0]["canonical_hex"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
    wire["owner"] = json!(owner(i));
    wire["owner_pubkey"] =
        json!(STANDARD.encode(hex::decode(key(i)["public_key_hex"].as_str().unwrap()).unwrap()));
    wire["genesis_hash"] = v["context"]["genesis_hash"].clone();
    wire["side"] = json!(side);
    wire["max_qty_lots"] = json!(q.to_string());
    wire["limit_price_ticks"] = json!(p.to_string());
    wire["order_id"] = json!(hex::encode([id; 32]));
    wire["max_fee_bps"] = json!("25");
    let raw = codec::Codec::default().encode("OrderV1", &wire).unwrap();
    let seed: [u8; 32] = hex::decode(key(i)["test_seed_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let (_, sk) = ml_dsa_65::KG::keygen_from_seed(&seed);
    let sig = sk
        .try_sign_with_seed(&[31; 32], &codec::frame("NUS/ORDER/V1", &raw), &[])
        .unwrap()
        .to_vec();
    (raw, sig)
}
pub fn signed(raw: &[u8], sig: &[u8], i: usize) -> Command {
    Command::Signed {
        kind: "ORDER".into(),
        raw: raw.to_vec(),
        signature: sig.to_vec(),
        session_owner: owner(i),
    }
}
pub fn record_command(
    before: &Value,
    r: &Value,
    objects: &nus_exchange_contract::s3::evidence::Objects,
) -> Command {
    let after = schema::decode("EngineState", &schema::bytes(&r["state_json"]).unwrap()).unwrap();
    match r["command_kind"].as_str().unwrap() {
        k @ ("ORDER" | "CANCEL") => {
            let raw = schema::bytes(&r["request_wire"]).unwrap();
            let owner = codec::Codec::default()
                .decode(if k == "ORDER" { "OrderV1" } else { "CancelV1" }, &raw)
                .unwrap()["owner"]
                .as_str()
                .unwrap()
                .to_owned();
            Command::Signed {
                kind: k.into(),
                raw,
                signature: schema::bytes(&r["signature"]).unwrap(),
                session_owner: owner,
            }
        }
        k @ ("WITHDRAW_PREPARE" | "WITHDRAW_ABORT") => Command::Local {
            kind: k.into(),
            raw: schema::bytes(&r["request_wire"]).unwrap(),
            session_owner: after["bindings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["first_command_seq"] == r["command_seq"] && x["kind"] == k)
                .unwrap()["owner"]
                .as_str()
                .unwrap()
                .into(),
        },
        "SNAPSHOT" => Command::Snapshot(canonical(&r["snapshot"]).unwrap()),
        "SEAL_BATCH" => Command::Seal(
            after["batches"].as_array().unwrap().last().unwrap()["seal_purpose"]
                .as_str()
                .unwrap()
                .into(),
        ),
        "ATTEMPT" | "RESOLVE_ATTEMPT" | "VOID_BATCH" => {
            let added = after["resolution_receipts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| {
                    !before["resolution_receipts"]
                        .as_array()
                        .unwrap()
                        .contains(x)
                });
            if let Some(v) = added {
                return Command::Receipt(v.clone());
            }
            if r["command_kind"] == "VOID_BATCH" {
                return Command::RejectFinal;
            }
            let prev = before["attempt_refs"].as_array().unwrap();
            let a = after["attempt_refs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| !prev.contains(x))
                .unwrap();
            let v = objects.typed(a, "Attempt").unwrap();
            if r["command_kind"] == "ATTEMPT" {
                Command::Attempt(v)
            } else {
                Command::Resolve(v)
            }
        }
        "CORRECTION" | "SETTLEMENT_APPLY" => Command::Apply,
        _ => panic!("unknown command"),
    }
}
pub fn bundle(i: &Inputs) -> Value {
    json!({"runtime_manifest":STANDARD.encode(&i.runtime_manifest),"files":i.files.iter().map(|(p,b)|(p.clone(),STANDARD.encode(b))).collect::<BTreeMap<_,_>>(),"guard":STANDARD.encode(&i.guard),"genesis":STANDARD.encode(&i.genesis)})
}
pub fn evidence(name: &str, v: &Value) {
    if let Some(p) = std::env::var_os("NUS70_EVIDENCE_DIR") {
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            Path::new(&p).join(format!("{name}.json")),
            serde_json::to_vec_pretty(v).unwrap(),
        )
        .unwrap();
    }
}
pub fn copy_home(name: &str, home: &Path) {
    if let Some(root) = std::env::var_os("NUS70_EVIDENCE_DIR") {
        let id = home
            .ancestors()
            .nth(4)
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let target = Path::new(&root).join("stores").join(format!("{name}-{id}"));
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap() {
                let e = e.unwrap();
                let ty = e.file_type().unwrap();
                assert!(!ty.is_symlink());
                if ty.is_dir() {
                    copy(&e.path(), &to.join(e.file_name()));
                } else {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
        }
        copy(home, &target);
    }
}

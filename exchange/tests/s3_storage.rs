//! rc3 storage shapes and real object IO. No dedicated-space/ACK/chain claim.
use base64::{Engine, engine::general_purpose::STANDARD};
use nus_exchange_contract::s3::{
    capacity::{self, Bounds},
    evidence::{self, Objects, RPC, TX, TYPED},
    journal::{self, Journal},
    schema,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
};
static INPUT: LazyLock<Value> = LazyLock::new(|| {
    let output = Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/evidence/s3-storage/oracle_inputs.py"
        ))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
});
fn objects() -> Objects {
    let mut objects = Objects::default();
    for v in INPUT["objects"].as_array().unwrap() {
        let r = objects
            .insert(
                &STANDARD.decode(v["raw_b64"].as_str().unwrap()).unwrap(),
                v["ref"]["media_type"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(r, v["ref"]);
    }
    objects
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(
            std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join(format!(
                    "s3-objects-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::SeqCst)
                )),
        )
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn context() -> Value {
    json!({"service_schema":"s3/3","chain_id":"nus-s3-dev-1","genesis_hash":"01".repeat(32),"contract_hash":"02".repeat(32),"config_hash":"03".repeat(32),"market_id":"DEVBASE/DEVQUOTE","market_config_version":"1"})
}
#[test]
fn checked_rust_capacity_matches_approved_python_oracle() {
    let objects = objects();
    for c in INPUT["cases"].as_array().unwrap() {
        let actual = capacity::certificate(&c["state"], &objects).unwrap();
        let mut expected = c["certificate"].clone();
        for v in expected.as_object_mut().unwrap().values_mut() {
            if let Some(n) = v.as_u64() {
                *v = json!(n.to_string());
            }
        }
        assert_eq!(actual.value(), expected, "{}", c["name"]);
        assert!(journal::canonical(&c["state"]).unwrap().len() as u128 <= actual.max_state_bytes);
        if actual.admissible() {
            assert_eq!(actual.additional(actual.reserved_bytes).unwrap(), 0);
            assert_eq!(actual.additional(actual.reserved_bytes - 1).unwrap(), 1);
        } else {
            assert_eq!(
                actual.additional(u128::MAX).unwrap_err(),
                "STORAGE_CAPACITY"
            );
        }
        println!(
            "S3_STORAGE {}",
            json!({"case":c["name"],"scope":"SERIALIZATION_BOUND_NO_ALLOCATION","certificate":actual.value(),"expected_diff":[],"result":"PASS"})
        );
    }
    assert_eq!(
        Bounds::new(u128::MAX, u128::MAX)
            .size("CommandResult")
            .unwrap_err(),
        "STORAGE_CAPACITY"
    );
    assert!(Bounds::new(1, 1).size("EngineState").is_err());
}
#[test]
fn unicode_counts_codepoints_and_matches_ascii_canonical_bytes() {
    let samples = [
        "A",
        "\"",
        "\\",
        "\0",
        "\u{1f}",
        "\n",
        "\u{80}",
        "한",
        "\u{ffff}",
        "\u{10000}",
        "😀",
        "\u{10ffff}",
        "A한😀\0\"\\\n",
    ];
    for sample in samples {
        for count in [0, 1, 255, 256] {
            let s: String = sample.chars().cycle().take(count).collect();
            let value = json!(s);
            schema::validate("Text", &value).unwrap();
            let raw = journal::canonical(&value).unwrap();
            assert!(raw.is_ascii());
            assert!(raw.len() as u128 <= Bounds::new(0, 0).size("Text").unwrap());
            assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), value);
        }
        assert!(
            schema::validate(
                "Text",
                &json!(sample.chars().next().unwrap().to_string().repeat(257))
            )
            .is_err()
        );
    }
    assert_eq!(
        journal::canonical(&json!("😀")).unwrap(),
        br#""\ud83d\ude00""#
    );
    assert_eq!(
        journal::canonical(&json!("\u{7f}")).unwrap(),
        br#""\u007f""#
    );
    assert_eq!(
        journal::canonical(&json!("😀".repeat(256))).unwrap().len(),
        3074
    );
}
#[test]
fn exact_three_mib_rpc_survives_disk_and_transitive_resolution() {
    let objects = objects();
    let step = &INPUT["large_step"];
    let refs = objects
        .graph(&json!([step["after_state"], step["result"]]))
        .unwrap();
    assert_eq!(json!(refs), step["journal_record"]["evidence_refs"]);
    assert_eq!(
        journal::canonical(&step["journal_record"]).unwrap().len(),
        99671
    );
    assert_eq!(
        schema::hash("NUS/S3/ENGINE_STATE/V1", &step["after_state"]).unwrap(),
        step["after_state_hash"]
    );
    let d = Dir::new();
    let mut journal = Journal::create(&d.0, context()).unwrap();
    for r in &refs {
        let media = r["media_type"].as_str().unwrap();
        assert_eq!(
            journal
                .store_evidence(objects.resolve(r, media).unwrap(), media)
                .unwrap(),
            *r
        );
    }
    let loaded = journal.load_objects(&refs).unwrap();
    loaded
        .verify_exact_refs(&json!([step["after_state"], step["result"]]), &json!(refs))
        .unwrap();
    let large =
        &step["after_state"]["resolution_receipts"][0]["terminal_tx"]["raw_results_response_ref"];
    let raw = journal.read_evidence(large).unwrap();
    assert_eq!(raw.len(), 3145728);
    assert_eq!(
        journal::sha256(&raw),
        "3a6bc3ad4851b52ca33f56f5fa632b23722ee6eaf066e4682cc7aac4978d31d2"
    );
    drop(journal);
    for _ in 0..2 {
        let (j, records) = Journal::open(&d.0, context()).unwrap();
        assert!(records.is_empty());
        assert_eq!(j.read_evidence(large).unwrap(), raw);
    }
}
#[test]
fn graph_requires_exact_transitive_refs_and_typed_slots() {
    let objects = objects();
    let step = &INPUT["large_step"];
    let root = json!([step["after_state"], step["result"]]);
    let refs = objects.graph(&root).unwrap();
    for i in 0..refs.len() {
        let mut bad = refs.clone();
        bad.remove(i);
        assert!(objects.verify_exact_refs(&root, &json!(bad)).is_err());
    }
    let mut duplicate = refs.clone();
    duplicate.push(refs[0].clone());
    assert!(objects.verify_exact_refs(&root, &json!(duplicate)).is_err());
    let mut reversed = refs.clone();
    reversed.reverse();
    assert!(objects.verify_exact_refs(&root, &json!(reversed)).is_err());
    let mut wrong = root.clone();
    wrong[0]["resolution_receipts"][0]["terminal_tx"]["raw_tx_ref"]["media_type"] = json!(RPC);
    assert!(objects.graph(&wrong).is_err());
    let mut arbitrary = objects.clone();
    let raw = journal::canonical(&json!({"looks_like":"metadata"})).unwrap();
    let r = arbitrary.insert(&raw, TYPED).unwrap();
    let root = json!({"attempt_refs":[r]});
    assert!(arbitrary.graph(&root).is_err());
}
#[test]
fn raw_object_caps_and_json_errors_fail_before_adoption() {
    for media in [RPC, TX, TYPED] {
        let size = evidence::limit(media).unwrap();
        let raw = if media == TX {
            vec![1; size]
        } else {
            let mut r = vec![b' '; size];
            r[0] = b'{';
            r[1] = b'}';
            r
        };
        assert!(evidence::reference(&raw, media).is_ok());
        assert!(evidence::reference(&vec![b' '; size + 1], media).is_err());
        assert!(evidence::reference(&[], media).is_err());
    }
    let mut o = Objects::default();
    let mut raw = b"{\"raw\":true}".to_vec();
    raw.resize(16_777_216, b' ');
    let r = o.insert(&raw, RPC).unwrap();
    assert_eq!(o.resolve(&r, RPC).unwrap(), raw);
    for raw in [
        b"{\"x\":1,\"x\":2}".as_slice(),
        b"{\"x\":NaN}",
        b"{\"x\":Infinity}",
        b"\xff",
        b"{bad",
    ] {
        assert!(o.insert(raw, RPC).is_err());
    }
    assert!(o.insert(b"{ }", TYPED).is_err());
    let r = o.insert(b"{}", RPC).unwrap();
    assert!(o.insert(b"{}", TX).is_err());
    let mut bad = r.clone();
    bad["byte_length"] = json!("02");
    assert!(o.resolve(&bad, RPC).is_err());
    let mut bad = r;
    bad["sha256"] = json!("../escape");
    assert!(o.resolve(&bad, RPC).is_err());
}
#[test]
fn disk_role_and_bytes_are_immutable_across_restart() {
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let r = j.store_evidence(b"{}", RPC).unwrap();
    drop(j);
    let (mut j, _) = Journal::open(&d.0, context()).unwrap();
    assert!(j.store_evidence(b"{}", TX).is_err());
    drop(j);
    let object =
        d.0.join("objects/sha256")
            .join(r["sha256"].as_str().unwrap());
    fs::write(&object, b"[]").unwrap();
    let (j, _) = Journal::open(&d.0, context()).unwrap();
    assert!(j.read_evidence(&r).is_err());
    assert_eq!(fs::read(&object).unwrap(), b"[]");
}
#[cfg(unix)]
#[test]
fn symlink_and_public_permissions_cannot_be_adopted() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for target in ["raw", "descriptor", "directory", "permissions"] {
        let d = Dir::new();
        let mut j = Journal::create(&d.0, context()).unwrap();
        let r = j.store_evidence(b"{}", RPC).unwrap();
        let object =
            d.0.join("objects/sha256")
                .join(r["sha256"].as_str().unwrap());
        let path = match target {
            "descriptor" => object.with_extension("ref"),
            "directory" => d.0.join("objects/sha256"),
            _ => object,
        };
        if target == "permissions" {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        } else {
            let saved = path.with_extension("saved");
            fs::rename(&path, &saved).unwrap();
            symlink(saved, &path).unwrap();
        }
        assert!(j.read_evidence(&r).is_err(), "{target}");
    }
}
#[test]
fn prior_schema_and_unordered_evidence_are_rejected() {
    for version in ["s3/1", "s3/2", "s2/1"] {
        let d = Dir::new();
        let mut c = context();
        c["service_schema"] = json!(version);
        assert!(Journal::create(&d.0, c).is_err());
        assert!(!d.0.exists());
    }
    let d = Dir::new();
    let mut j = Journal::create(&d.0, context()).unwrap();
    let mut refs = vec![
        j.store_evidence(b"{}", RPC).unwrap(),
        j.store_evidence(b"[]", RPC).unwrap(),
    ];
    refs.sort_by(|a, b| b["sha256"].as_str().cmp(&a["sha256"].as_str()));
    let r = json!({"context":context(),"command_seq":"1","previous_commit_hash":schema::ZERO,"command_kind":"ORDER","evidence_refs":refs});
    assert!(j.append(&r, 4096, false).is_err());
    assert_eq!(j.commit().command_seq, 0);
}

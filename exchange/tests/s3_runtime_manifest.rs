#![cfg(feature = "dev-local-demo")]
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
        dev_local::{Command, Engine, Inputs, Validated},
        journal::{canonical, sha256},
        schema,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command as Process};

const PUBLIC: &str = "proposals/s3-local-account-receipt-v1/";
const PINS: [&str; 3] = [
    "public_receipt_manifest_sha256",
    "public_receipt_schema_sha256",
    "public_receipt_version",
];
fn key(i: usize) -> (String, Vec<u8>, ml_dsa_65::PrivateKey) {
    let (pk, sk) = ml_dsa_65::KG::keygen_from_seed(&[210 + i as u8; 32]);
    let pk = pk.into_bytes().to_vec();
    (STANDARD.encode(codec::address(&pk).unwrap()), pk, sk)
}
fn fresh(bps: u32) -> (Inputs, Value) {
    let (_, mut v) = fixture::initial(bps);
    for (n, a) in v["accounts"].as_array_mut().unwrap().iter_mut().enumerate() {
        let (owner, pk, _) = key(n);
        a["owner"] = json!(owner);
        a["public_key"] = json!(STANDARD.encode(pk));
    }
    v["accounts"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|a| schema::bytes(&a["owner"]).unwrap());
    let input = fixture::inputs(bps, v["accounts"].as_array().unwrap());
    v["context"] = Validated::new(input.clone()).unwrap().context().clone();
    fixture::finish(&mut v);
    (input, v)
}
fn signed(v: &Value) -> (Command, Vec<u8>, Vec<u8>) {
    let (template, _) = fixture::sign_order(v, 0, "2", 1000, 10000, 94);
    let mut wire = codec::Codec::default()
        .decode("OrderV1", &template)
        .unwrap();
    let (owner, pk, sk) = key(0);
    wire["owner"] = json!(owner);
    wire["owner_pubkey"] = json!(STANDARD.encode(pk));
    let raw = codec::Codec::default().encode("OrderV1", &wire).unwrap();
    let sig = sk
        .try_sign_with_seed(&[94; 32], &codec::frame("NUS/ORDER/V1", &raw), &[])
        .unwrap()
        .to_vec();
    (
        Command::Signed {
            kind: "ORDER".into(),
            raw: raw.clone(),
            signature: sig.clone(),
            session_owner: owner,
        },
        raw,
        sig,
    )
}
fn disk(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, p: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(p).unwrap() {
            let p = entry.unwrap().path();
            let name = p.strip_prefix(root).unwrap().to_str().unwrap().to_string();
            if p.is_dir() {
                out.insert(name, "directory".into());
                walk(root, &p, out);
            } else {
                out.insert(name, sha256(&fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
// An attacker can recompute untrusted transport hashes. The fixed approved
// pins and original inherited bytes must still reject the changed source.
fn repin(i: &mut Inputs, m: &Value) {
    i.runtime_manifest = canonical(m).unwrap();
    i.approved_runtime_sha256 = sha256(&i.runtime_manifest);
    let mut g: Value = serde_json::from_slice(&i.guard).unwrap();
    g["runtime_manifest_sha256"] = json!(i.approved_runtime_sha256);
    i.guard = canonical(&g).unwrap();
}
fn reseal(i: &mut Inputs, m: &mut Value) {
    m["files_sha256"] = json!(
        i.files
            .iter()
            .map(|(p, b)| (p.clone(), sha256(b)))
            .collect::<BTreeMap<_, _>>()
    );
    m["contract_sha256"] = json!(fixture::aggregate(&i.files));
    let mut genesis: Value = serde_json::from_slice(&i.genesis).unwrap();
    genesis["app_state"]["contract_hash"] = m["contract_sha256"].clone();
    i.genesis = canonical(&genesis).unwrap();
    let mut g: Value = serde_json::from_slice(&i.guard).unwrap();
    g["context"]["contract_hash"] = m["contract_sha256"].clone();
    g["context"]["genesis_hash"] = json!(sha256(&i.genesis));
    i.guard = canonical(&g).unwrap();
    repin(i, m);
}

#[test]
fn runtime_nine_fields_pins_and_source_mutations_reject_before_home() {
    for bps in [0, 25] {
        let (input, bootstrap) = fresh(bps);
        let home = fixture::home(bps);
        let parent = home.parent().unwrap();
        let before = disk(parent);
        let original: Value = serde_json::from_slice(&input.runtime_manifest).unwrap();
        assert_eq!(original.as_object().unwrap().len(), 9);
        let mut cases: Vec<(String, Inputs)> = vec![];
        for pin in PINS {
            for change in ["missing", "wrong", "null", "type", "case"] {
                let mut i = input.clone();
                let mut m = original.clone();
                match change {
                    "missing" => {
                        m.as_object_mut().unwrap().remove(pin);
                    }
                    "wrong" => m[pin] = json!("invalid"),
                    "null" => m[pin] = Value::Null,
                    "type" => m[pin] = json!([m[pin].clone()]),
                    "case" => m[pin] = json!(m[pin].as_str().unwrap().to_uppercase()),
                    _ => unreachable!(),
                }
                repin(&mut i, &m);
                cases.push((format!("{pin}-{change}"), i));
            }
        }
        for case in [
            "old-six",
            "extra",
            "duplicate",
            "contract",
            "genesis-contract",
            "guard-contract",
            "raw-manifest",
            "raw-schema",
            "manifest-resealed",
            "schema-resealed",
            "version-bytes-resealed",
            "manifest-omitted",
            "overlay-omitted",
        ] {
            let mut i = input.clone();
            let mut m = original.clone();
            match case {
                "old-six" => {
                    for pin in PINS {
                        m.as_object_mut().unwrap().remove(pin);
                    }
                }
                "extra" => m["unknown"] = json!(false),
                "contract" => m["contract_sha256"] = json!("0".repeat(64)),
                "genesis-contract" => {
                    let mut g: Value = serde_json::from_slice(&i.genesis).unwrap();
                    g["app_state"]["contract_hash"] = json!("0".repeat(64));
                    i.genesis = canonical(&g).unwrap();
                }
                "guard-contract" => {
                    let mut g: Value = serde_json::from_slice(&i.guard).unwrap();
                    g["context"]["contract_hash"] = json!("0".repeat(64));
                    i.guard = canonical(&g).unwrap();
                }
                "raw-manifest" | "manifest-resealed" => {
                    i.files
                        .get_mut(&format!("{PUBLIC}MANIFEST.json"))
                        .unwrap()
                        .push(b' ');
                }
                "raw-schema" | "schema-resealed" => {
                    i.files
                        .get_mut(&format!("{PUBLIC}schema.json"))
                        .unwrap()
                        .push(b' ');
                }
                "version-bytes-resealed" => {
                    let p = format!("{PUBLIC}schema.json");
                    let s = String::from_utf8(i.files[&p].clone())
                        .unwrap()
                        .replace("s3-dev-local-account/1", "s3-dev-local-account/2");
                    i.files.insert(p, s.into_bytes());
                }
                "manifest-omitted" => {
                    i.files.remove(&format!("{PUBLIC}MANIFEST.json"));
                }
                "overlay-omitted" => {
                    i.files.retain(|p, _| !p.starts_with(PUBLIC));
                }
                "duplicate" => {}
                _ => unreachable!(),
            }
            if case.ends_with("resealed") || case.ends_with("omitted") {
                reseal(&mut i, &mut m);
            } else {
                repin(&mut i, &m);
            }
            if case == "duplicate" {
                i.runtime_manifest.splice(
                    1..1,
                    b"\"public_receipt_version\":\"s3-dev-local-account/1\","
                        .iter()
                        .copied(),
                );
                i.approved_runtime_sha256 = sha256(&i.runtime_manifest);
            }
            cases.push((case.into(), i));
        }
        let mut result = vec![];
        for (case, i) in cases {
            let err = Validated::new(i).err().expect(&case);
            assert_eq!(disk(parent), before, "{case}");
            assert!(!home.exists());
            result.push(json!({"case":case,"error":err.to_string(),"home_created":false,"files_changed":false}));
        }
        let mut bad = bootstrap.clone();
        bad["context"]["contract_hash"] = json!("0".repeat(64));
        fixture::finish(&mut bad);
        assert!(
            Engine::create(
                &home,
                Validated::new(input).unwrap(),
                &canonical(&bad).unwrap()
            )
            .is_err()
        );
        assert_eq!(disk(parent), before);
        assert!(!home.exists());
        fixture::evidence(
            &format!("runtime-negative-fee{bps}"),
            &json!({"cases":result,"bad_bootstrap_files_changed":false,"result":"PASS"}),
        );
    }
}

#[test]
fn runtime_context_signed_query_and_public_bytes_survive_two_restarts() {
    for bps in [0, 25] {
        let (i, v) = fresh(bps);
        let h = fixture::home(bps);
        let c = Validated::new(i.clone()).unwrap();
        let expected = c.context().clone();
        let mut old = i.files.clone();
        old.remove(&format!("{PUBLIC}MANIFEST.json"));
        assert_ne!(expected["contract_hash"], fixture::aggregate(&old));
        let e = Engine::create(&h, c.clone(), &canonical(&v).unwrap()).unwrap();
        let (command, raw, sig) = signed(&v);
        let obs = fixture::observation(&v);
        let receipt = e
            .execute(command, &[], &obs, fixture::NOW)
            .unwrap()
            .unwrap();
        assert_eq!(receipt["command_result"]["code"], "OK");
        let view = e.reader().get().unwrap();
        assert_eq!(view.state["context"], expected);
        let public = e
            .account_receipt(view.commit.command_seq, &key(0).0)
            .unwrap()
            .unwrap();
        assert_eq!(public.to_value()["context"], expected);
        let before = disk(&h);
        drop(e);
        for _ in 0..2 {
            let e = Engine::open(&h, c.clone()).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state, view.state);
            assert_eq!(after.commit, view.commit);
            assert_eq!(after.receipts, view.receipts);
            assert_eq!(
                e.query_signed("ORDER", &raw, &sig, &key(0).0, &obs, fixture::NOW)
                    .unwrap(),
                Some(receipt.clone())
            );
            assert!(
                e.query_signed("ORDER", &raw, &sig, &key(1).0, &obs, fixture::NOW)
                    .is_err()
            );
            let (command, _, _) = signed(&v);
            assert_eq!(
                e.execute(command, &[], &obs, fixture::NOW).unwrap(),
                Some(receipt.clone())
            );
            assert_eq!(
                e.account_receipt(view.commit.command_seq, &key(0).0)
                    .unwrap()
                    .unwrap()
                    .as_bytes(),
                public.as_bytes()
            );
            assert_eq!(e.reader().get().unwrap().commit, view.commit);
            assert_eq!(disk(&h), before);
        }
        fixture::copy_home(&format!("runtime-fee{bps}"), &h);
        fixture::evidence(
            &format!("runtime-replay-fee{bps}"),
            &json!({"input_bundle":fixture::bundle(&i),"test_pin":i.approved_runtime_sha256,"bootstrap":v,"trusted_receipt":receipt,"public_receipt":public.to_value(),"state":view.state,"receipt_ledger":view.receipts,"disk":before,"restarts":2,"diff":[],"scope":"SYNTHETIC_COMPONENT_NOT_APPROVED_RUNTIME","result":"PASS"}),
        );
    }
}

#[test]
fn runtime_disk_pin_tampering_closes_query_and_restart_without_repair() {
    for bps in [0, 25] {
        let (i, v) = fresh(bps);
        let c = Validated::new(i.clone()).unwrap();
        let mut results = vec![];
        for field in PINS.into_iter().chain(["contract_sha256"]) {
            let h = fixture::home(bps);
            let e = Engine::create(&h, c.clone(), &canonical(&v).unwrap()).unwrap();
            let (command, _, _) = signed(&v);
            e.execute(command, &[], &fixture::observation(&v), fixture::NOW)
                .unwrap();
            let view = e.reader().get().unwrap();
            let mut m: Value = serde_json::from_slice(&i.runtime_manifest).unwrap();
            m[field] = json!("bad");
            fs::write(h.join("runtime.dev.json"), canonical(&m).unwrap()).unwrap();
            let before = disk(&h);
            assert!(
                e.account_receipt(view.commit.command_seq, &key(0).0)
                    .is_err()
            );
            let (command, _, _) = signed(&v);
            assert!(
                e.execute(command, &[], &fixture::observation(&v), fixture::NOW)
                    .is_err()
            );
            assert_eq!(e.reader().get().unwrap().state, view.state);
            assert_eq!(e.reader().get().unwrap().commit, view.commit);
            drop(e);
            for _ in 0..2 {
                assert!(Engine::open(&h, c.clone()).is_err());
                assert_eq!(disk(&h), before);
            }
            results.push(json!({"field":field,"query_rejected":true,"new_commit":false,"restarts_rejected":2,"repair":false}));
        }
        fixture::evidence(
            &format!("runtime-disk-tamper-fee{bps}"),
            &json!({"cases":results,"result":"PASS"}),
        );
    }
}

#[test]
fn runtime_cli_failed_validation_cannot_publish_or_replace_a_home() {
    for bps in [0, 25] {
        let (i, v) = fresh(bps);
        let h = fixture::home(bps);
        let p = h.parent().unwrap();
        let input = p.join("input.json");
        let profile = p.join("profile.json");
        let boot = p.join("bootstrap.json");
        fs::write(&input, canonical(&fixture::bundle(&i)).unwrap()).unwrap();
        fs::write(&profile, &i.effective_profile).unwrap();
        fs::write(&boot, canonical(&v).unwrap()).unwrap();
        let invoke = |mode: &str, pin: &str| {
            let mut cmd = Process::new(env!("CARGO_BIN_EXE_nus-s3-local-demo"));
            cmd.arg(mode)
                .arg("--input-set")
                .arg(&input)
                .arg("--local-demo-profile")
                .arg(&profile)
                .arg("--runtime-pin")
                .arg(pin)
                .arg("--acknowledge-unproven-space")
                .arg("--home")
                .arg(&h);
            if mode == "create" {
                cmd.arg("--bootstrap").arg(&boot);
            }
            cmd.output().unwrap()
        };
        let before = disk(p);
        let out = invoke("create", &"0".repeat(64));
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(disk(p), before);
        assert!(!h.exists());
        assert!(
            invoke("validate", &i.approved_runtime_sha256)
                .status
                .success()
        );
        assert_eq!(disk(p), before);
        let created = invoke("create", &i.approved_runtime_sha256);
        assert!(
            created.status.success(),
            "{}",
            String::from_utf8_lossy(&created.stderr)
        );
        let before = disk(p);
        assert!(
            !invoke("create", &i.approved_runtime_sha256)
                .status
                .success()
        );
        assert_eq!(disk(p), before);
        for _ in 0..2 {
            let opened = invoke("open", &i.approved_runtime_sha256);
            assert!(opened.status.success());
            assert_eq!(opened.stdout, created.stdout);
            assert_eq!(disk(p), before);
        }
        fixture::evidence(
            &format!("runtime-cli-fee{bps}"),
            &json!({"invalid_pin_stderr":String::from_utf8_lossy(&out.stderr),"create_stdout":String::from_utf8_lossy(&created.stdout),"restarts":2,"no_replace":true,"files_changed_on_failure":false,"result":"PASS"}),
        );
    }
}

/// Run explicitly after the review harness exports actual B InputBundle bytes.
/// Synthetic same-height snapshot; no RPC, chain execution, or runtime approval.
#[test]
#[ignore = "requires NUS_BUNDLE_EXPORT from B's actual InputBundle/initializer"]
fn actual_b_bundle_bootstrap_context_queries_and_two_restarts() {
    let export = std::path::PathBuf::from(std::env::var_os("NUS_BUNDLE_EXPORT").expect("B export"));
    for bps in [0, 25] {
        let root = export.join(format!("fee{bps}"));
        let runtime = fs::read(root.join("runtime.json")).unwrap();
        let c = Validated::decode_bundle(
            &fs::read(root.join("bundle.json")).unwrap(),
            fs::read(root.join("profile.json")).unwrap(),
            sha256(&runtime),
            true,
        )
        .unwrap();
        let genesis: Value =
            serde_json::from_slice(&fs::read(root.join("genesis.json")).unwrap()).unwrap();
        let guard: Value =
            serde_json::from_slice(&fs::read(root.join("guard.json")).unwrap()).unwrap();
        let result: Value =
            serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
        assert_eq!(c.context(), &result["B_context"]);
        let (_, mut snapshot) = fixture::initial(bps);
        let mut rows = vec![];
        for public_key in genesis["app_state"]["public_keys"].as_array().unwrap() {
            let mut row = snapshot["accounts"][0].clone();
            row["public_key"] = public_key.clone();
            row["owner"] = json!(
                STANDARD.encode(codec::address(&schema::bytes(public_key).unwrap()).unwrap())
            );
            rows.push(row);
        }
        rows.sort_by_key(|a| schema::bytes(&a["owner"]).unwrap());
        snapshot["accounts"] = json!(rows);
        snapshot["context"] = c.context().clone();
        snapshot["operator"] = json!(
            STANDARD.encode(
                codec::address(
                    &schema::bytes(&genesis["app_state"]["settlement_operator_public_keys"][0])
                        .unwrap()
                )
                .unwrap()
            )
        );
        for n in 0..2 {
            let confirmed: u128 = rows
                .iter()
                .map(|r| codec::integer(&r["assets"][n]["confirmed_atoms"], 128).unwrap())
                .sum();
            let bank: u128 = rows
                .iter()
                .map(|r| codec::integer(&r["assets"][n]["bank_atoms"], 128).unwrap())
                .sum();
            for f in ["module_bank_atoms", "sum_confirmed_atoms"] {
                snapshot["assets"][n][f] = json!(confirmed.to_string());
            }
            snapshot["assets"][n]["supply_atoms"] = json!((confirmed + bank).to_string());
        }
        fixture::finish(&mut snapshot);
        let placeholder = fixture::home(bps);
        let h = placeholder
            .parent()
            .unwrap()
            .with_file_name(guard["run_uuid"].as_str().unwrap())
            .join(format!("fee{bps}"));
        fs::create_dir_all(h.parent().unwrap()).unwrap();
        let e = Engine::create(&h, c.clone(), &canonical(&snapshot).unwrap()).unwrap();
        let principal = rows[0]["owner"].as_str().unwrap();
        let command = || Command::Local {
            kind: "WITHDRAW_PREPARE".into(),
            raw: canonical(&json!({"request_id":"94".repeat(32)})).unwrap(),
            session_owner: principal.into(),
        };
        let receipt = e
            .execute(
                command(),
                &[],
                &fixture::observation(&snapshot),
                fixture::NOW,
            )
            .unwrap()
            .unwrap();
        assert_eq!(receipt["command_result"]["code"], "OK");
        let before = e.reader().get().unwrap();
        let seq = before.commit.command_seq;
        let public = e.account_receipt(seq, principal).unwrap().unwrap();
        assert_eq!(public.to_value()["context"], result["B_context"]);
        let files = disk(&h);
        drop(e);
        for _ in 0..2 {
            let e = Engine::open(&h, c.clone()).unwrap();
            let view = e.reader().get().unwrap();
            assert_eq!(view.state, before.state);
            assert_eq!(view.commit, before.commit);
            assert_eq!(view.receipts, before.receipts);
            assert_eq!(
                e.account_receipt(seq, principal)
                    .unwrap()
                    .unwrap()
                    .as_bytes(),
                public.as_bytes()
            );
            assert!(
                e.account_receipt(seq, rows[1]["owner"].as_str().unwrap())
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                e.execute(
                    command(),
                    &[],
                    &fixture::observation(&snapshot),
                    fixture::NOW
                )
                .unwrap(),
                Some(receipt.clone())
            );
            assert_eq!(e.reader().get().unwrap().commit, before.commit);
            assert_eq!(disk(&h), files);
        }
        fixture::copy_home(&format!("actual-b-runtime-fee{bps}"), &h);
        fixture::evidence(
            &format!("actual-b-replay-fee{bps}"),
            &json!({"B_context":result["B_context"],"bootstrap":snapshot,"trusted_receipt":receipt,"public_receipt":public.to_value(),"state":before.state,"receipt_ledger":before.receipts,"disk":files,"restarts":2,"diff":[],"scope":"B_INPUT_C_STORE_WITH_SYNTHETIC_SNAPSHOT","result":"PASS"}),
        );
    }
}

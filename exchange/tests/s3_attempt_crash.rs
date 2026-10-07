//! Offline F04 component verification. No listener, RPC, transport or service.
#![cfg(all(
    feature = "dev-local-demo",
    feature = "dev-local-settlement",
    feature = "fault-injection"
))]

#[path = "../../ops/s3-local/runtime/attempt_crash.rs"]
mod attempt_crash;
#[path = "support/dev_fixture.rs"]
mod fixture;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, Signer as _},
};
use nus_exchange_contract::s3::{
    dev_local::{Command, Engine, Error, Validated},
    journal::{canonical, sha256},
    schema,
    settlement_local::chain::{self, OperatorSigner},
    snapshot::{Binding, Snapshot},
};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};

struct TestSigner(Vec<u8>);
impl TestSigner {
    fn new() -> Self {
        Self(hex::decode(fixture::key(16)["public_key_hex"].as_str().unwrap()).unwrap())
    }
}
impl OperatorSigner for TestSigner {
    fn public_key(&self) -> &[u8] {
        &self.0
    }
    fn sign(&self, doc: &[u8]) -> nus_exchange_contract::s3::dev_local::Result<Vec<u8>> {
        let seed: [u8; 32] = hex::decode(fixture::key(16)["test_seed_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let (_, sk) = ml_dsa_65::KG::keygen_from_seed(&seed);
        Ok(sk.try_sign_with_seed(&[0; 32], doc, &[]).unwrap().to_vec())
    }
}

fn snapshot(value: &serde_json::Value, bps: u32) -> Snapshot {
    Binding::new(
        value["context"].clone(),
        value["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| schema::bytes(&a["owner"]).unwrap())
            .collect(),
        [4_000_000_000_000; 2],
        bps,
    )
    .unwrap()
    .decode(&canonical(value).unwrap())
    .unwrap()
}

fn seed(
    bps: u32,
) -> (
    PathBuf,
    nus_exchange_contract::s3::dev_local::Inputs,
    serde_json::Value,
) {
    let (inputs, value) = fixture::initial(bps);
    let home = fixture::home(bps);
    let engine = Engine::create(
        &home,
        Validated::new(inputs.clone()).unwrap(),
        &canonical(&value).unwrap(),
    )
    .unwrap();
    let observation = fixture::observation(&value);
    for (owner, side, id) in [(0, "2", 241), (1, "1", 242)] {
        let (raw, signature) = fixture::sign_order(&value, owner, side, 1000, 10000, id);
        engine
            .execute(
                fixture::signed(&raw, &signature, owner),
                &[],
                &observation,
                fixture::NOW,
            )
            .unwrap();
    }
    engine
        .execute(
            Command::Seal("NORMAL".into()),
            &[],
            &observation,
            fixture::NOW,
        )
        .unwrap();
    drop(engine);
    (home, inputs, value)
}

fn attempt_for(
    engine: &Engine,
    value: &serde_json::Value,
    bps: u32,
) -> (serde_json::Value, Vec<u8>) {
    let view = engine.reader().get().unwrap();
    let batch_id = view.state["batches"][0]["batch"]["batch_id"]
        .as_str()
        .unwrap();
    let current = snapshot(value, bps);
    let raw_batch = chain::sealed_batch(&view.state, batch_id).unwrap();
    chain::settle_attempt(&current, &raw_batch, 1, 16, 0, &TestSigner::new()).unwrap()
}

#[test]
fn f04_attempt_crash_child() {
    let Ok(home) = std::env::var("SRE_F04_HOME") else {
        return;
    };
    let bps: u32 = std::env::var("SRE_F04_FEE").unwrap().parse().unwrap();
    let evidence = PathBuf::from(std::env::var("SRE_F04_EVIDENCE").unwrap());
    let (inputs, value) = fixture::initial(bps);
    let engine = std::sync::Arc::new(
        Engine::open(Path::new(&home), Validated::new(inputs).unwrap()).unwrap(),
    );
    let (attempt, tx) = attempt_for(&engine, &value, bps);
    let result = attempt_crash::run_recorded(
        engine,
        attempt,
        tx,
        &fixture::observation(&value),
        fixture::NOW,
        &evidence,
    );
    panic!("F04 crash did not exit: {result:?}");
}

#[test]
fn f04_invalid_attempt_tx_binding_rejects_before_report_or_store() {
    for bps in [0, 25] {
        let (home, inputs, value) = seed(bps);
        let engine = std::sync::Arc::new(
            Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap(),
        );
        let before = engine.reader().get().unwrap();
        let wal = std::fs::read(home.join("journal.dev.wal")).unwrap();
        let marker = std::fs::read(home.join("commit.dev.json")).unwrap();
        let (mut attempt, tx) = attempt_for(&engine, &value, bps);
        attempt["tx_hash"] = serde_json::json!(schema::ZERO);
        let evidence = home.with_extension("f04-invalid");
        std::fs::create_dir(&evidence).unwrap();
        std::fs::set_permissions(&evidence, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            attempt_crash::run_recorded(
                engine.clone(),
                attempt,
                tx,
                &fixture::observation(&value),
                fixture::NOW,
                &evidence,
            ),
            Err(Error::Invalid("F04_ATTEMPT_INPUT"))
        ));
        assert_eq!(std::fs::read_dir(&evidence).unwrap().count(), 0);
        assert_eq!(engine.reader().get().unwrap().commit, before.commit);
        assert_eq!(std::fs::read(home.join("journal.dev.wal")).unwrap(), wal);
        assert_eq!(std::fs::read(home.join("commit.dev.json")).unwrap(), marker);
        drop(before);
        drop(engine);
        for _ in 0..2 {
            let reopened = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
            assert_eq!(reopened.reader().get().unwrap().commit.command_seq, 3);
        }
        std::fs::remove_dir_all(evidence).unwrap();
        std::fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn f04_after_attempt_wal_fsync_preserves_unknown_tail_and_creates_no_envelope() {
    for bps in [0, 25] {
        let (home, inputs, _value) = seed(bps);
        let marker_before = std::fs::read(home.join("commit.dev.json")).unwrap();
        let wal_before = std::fs::read(home.join("journal.dev.wal")).unwrap();
        let evidence = home.with_extension("f04-evidence");
        std::fs::create_dir(&evidence).unwrap();
        std::fs::set_permissions(&evidence, std::fs::Permissions::from_mode(0o700)).unwrap();

        let mut child = Process::new(std::env::current_exe().unwrap())
            .args(["--exact", "f04_attempt_crash_child", "--nocapture"])
            .env("SRE_F04_HOME", &home)
            .env("SRE_F04_FEE", bps.to_string())
            .env("SRE_F04_EVIDENCE", &evidence)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("F04 child timeout");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(attempt_crash::EXIT));

        let report = std::fs::read(evidence.join("storage-crash.jsonl")).unwrap();
        let rows: Vec<serde_json::Value> = std::str::from_utf8(&report)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["phase"], "reserved");
        assert_eq!(rows[0]["crash_verified"], false);
        let command = STANDARD
            .decode(rows[0]["command_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(rows[0]["command_sha256"], sha256(&command));
        let command: serde_json::Value = serde_json::from_slice(&command).unwrap();
        assert_eq!(command["schema"], "s3-local-f04-attempt-crash/1");
        assert_eq!(command["fault_id"], "F04");
        assert_eq!(command["command"], "Attempt");
        assert_eq!(command["point"], "after_wal_sync");
        assert_eq!(command["expected"], "UNKNOWN_TAIL_NO_NEW_ENVELOPE");
        assert_eq!(command["exit_code"], 86);

        let marker_after = std::fs::read(home.join("commit.dev.json")).unwrap();
        let wal_after = std::fs::read(home.join("journal.dev.wal")).unwrap();
        assert_eq!(marker_after, marker_before);
        assert!(wal_after.len() > wal_before.len());
        assert!(home.join("transaction.dev").exists());
        for _ in 0..2 {
            match Engine::open(&home, Validated::new(inputs.clone()).unwrap()) {
                // C sees the retained transaction marker before parsing the
                // longer WAL and closes with its generic unknown-store code.
                // The contractual outcome remains UNKNOWN_TAIL_NO_NEW_ENVELOPE.
                Err(Error::Recovery(code)) => assert_eq!(code, "UNKNOWN_OR_INCOMPLETE_STORE"),
                Err(error) => panic!("unexpected F04 reopen error: {error:?}"),
                Ok(_) => panic!("F04 unknown tail reopened"),
            }
            assert_eq!(
                std::fs::read(home.join("journal.dev.wal")).unwrap(),
                wal_after
            );
            assert_eq!(
                std::fs::read(home.join("commit.dev.json")).unwrap(),
                marker_after
            );
            assert_eq!(
                std::fs::read(evidence.join("storage-crash.jsonl")).unwrap(),
                report
            );
        }
        std::fs::remove_dir_all(evidence).unwrap();
        std::fs::remove_dir_all(home).unwrap();
    }
}

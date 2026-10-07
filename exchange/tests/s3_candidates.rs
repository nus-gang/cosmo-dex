//! Private S3 candidate tests with actual ML-DSA user/operator signatures and
//! synthetic raw JSON-RPC responses. No live chain or durable service ACK proof.
use base64::{Engine, engine::general_purpose::STANDARD};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, Signer},
};
use nus_exchange_contract::{
    codec::{self, Codec},
    s3::{
        journal::{canonical, sha256},
        ledger::Asset,
        schema,
        sequencer::Candidate,
        snapshot::{Binding, Observation},
    },
};
use serde_json::{Value, json};
const NOW: u64 = 1791193000000;
#[cfg(feature = "dev-local-demo")]
#[path = "support/dev_fixture.rs"]
mod dev_fixture;
#[cfg(all(feature = "dev-local-demo", feature = "fault-injection"))]
#[path = "support/f14_correction.rs"]
mod f14_correction;
#[cfg(feature = "dev-local-demo")]
#[path = "support/failure_recovery.rs"]
mod failure_recovery;
#[cfg(feature = "dev-local-demo")]
#[path = "support/seal_selection.rs"]
mod seal_selection;
#[cfg(feature = "dev-local-demo")]
#[path = "support/trusted_recovery.rs"]
mod trusted_recovery;
#[cfg(feature = "dev-local-demo")]
struct DevTrace {
    engine: Option<nus_exchange_contract::s3::dev_local::Engine>,
    config: nus_exchange_contract::s3::dev_local::Validated,
    home: std::path::PathBuf,
}
struct Trace {
    #[cfg(feature = "dev-local-demo")]
    dev: DevTrace,
    initial: Candidate,
    records: Vec<(Value, nus_exchange_contract::s3::evidence::Objects)>,
    commit: nus_exchange_contract::s3::journal::Commit,
}
thread_local! { static TRACE: std::cell::RefCell<Option<Trace>> = const { std::cell::RefCell::new(None) }; }
fn empty_commit() -> nus_exchange_contract::s3::journal::Commit {
    nus_exchange_contract::s3::journal::Commit {
        command_seq: 0,
        record_hash: schema::ZERO.into(),
        end_offset: 0,
    }
}
fn replay_check(before: &Candidate, after: Candidate, mut kind: &str) -> Candidate {
    use nus_exchange_contract::s3::{evidence::Objects, journal, record::Prepared};
    if kind == "SETTLEMENT_APPLY"
        && after.full_state().unwrap()["corrections"] != before.full_state().unwrap()["corrections"]
    {
        kind = "CORRECTION";
    }
    if kind == "VOID_BATCH" {
        let a = before.full_state().unwrap();
        let b = after.full_state().unwrap();
        if b["resolution_receipts"].as_array().unwrap().len()
            > a["resolution_receipts"].as_array().unwrap().len()
            && b["resolution_receipts"].as_array().unwrap().last().unwrap()["disposition"]
                == "COMMITTED"
        {
            kind = "RESOLVE_ATTEMPT";
        }
    }
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap();
        assert_eq!(
            t.commit.command_seq,
            before.sequence(),
            "trace must not skip transitions"
        );
        let prepared =
            Prepared::prepare(before, &after, kind, &observation(&after), NOW, &t.commit).unwrap();
        let full = after.evidence_set().unwrap();
        let mut objects = Objects::default();
        for r in prepared.record["evidence_refs"].as_array().unwrap() {
            let media = r["media_type"].as_str().unwrap();
            objects
                .insert(full.resolve(r, media).unwrap(), media)
                .unwrap();
        }
        if let Some(dir) = std::env::var_os("S3_CANDIDATE_EVIDENCE_DIR") {
            let dir = std::path::PathBuf::from(dir).join("objects");
            std::fs::create_dir_all(&dir).unwrap();
            for (r, raw) in objects.entries() {
                let path = dir.join(r["sha256"].as_str().unwrap());
                if path.exists() {
                    assert_eq!(std::fs::read(&path).unwrap(), raw);
                } else {
                    std::fs::write(&path, raw).unwrap();
                }
                std::fs::write(path.with_extension("ref.json"), canonical(r).unwrap()).unwrap();
            }
            evidence(
                &format!("bootstrap-{}", t.initial.snapshot().id()),
                t.initial.snapshot().value().clone(),
            );
        }
        #[cfg(feature = "dev-local-demo")]
        {
            let engine = t.dev.engine.as_ref().unwrap();
            let command = dev_fixture::record_command(
                &before.full_state().unwrap(),
                &prepared.record,
                &objects,
            );
            let raw = objects
                .entries()
                .map(|(r, b)| (b.to_vec(), r["media_type"].as_str().unwrap().to_owned()))
                .collect::<Vec<_>>();
            let result = engine
                .execute(command, &raw, &observation(&after), NOW)
                .unwrap()
                .unwrap();
            assert_eq!(result["command_result"], prepared.result);
            let view = engine.reader().get().unwrap();
            assert_eq!(view.state, after.full_state().unwrap());
            assert_eq!(view.commit.command_seq, after.sequence());
            assert_eq!(result["durable_ack"], false);
            if kind == "ATTEMPT" && after.attempts().last().unwrap()["kind"] == "SETTLE" {
                let before = engine.reader().get().unwrap();
                assert!(
                    engine
                        .execute(
                            nus_exchange_contract::s3::dev_local::Command::RejectFinal,
                            &[],
                            &observation(&after),
                            NOW
                        )
                        .is_err()
                );
                assert_eq!(before.state, engine.reader().get().unwrap().state);
                assert_eq!(before.commit, engine.reader().get().unwrap().commit);
            }

            if kind == "ATTEMPT" {
                let attempt = after.attempts().last().unwrap();
                let hash = attempt["tx_hash"].as_str().unwrap();
                let bytes = engine
                    .with_committed_attempt(hash, |stored, raw| {
                        assert_eq!(stored, attempt);
                        assert_eq!(sha256(raw), hash);
                        raw.len()
                    })
                    .unwrap()
                    .unwrap();
                assert!(bytes > 0);
            }
        }
        let (replayed, _) =
            Prepared::replay(before, &prepared.record, &objects, &t.commit).unwrap();
        assert_eq!(replayed.full_state().unwrap(), after.full_state().unwrap());
        let framed = journal::frame(&canonical(&prepared.record).unwrap()).unwrap();
        t.commit.command_seq = after.sequence();
        t.commit.record_hash = sha256(&framed);
        t.commit.end_offset += framed.len() as u64;
        t.records.push((prepared.record, objects));
    });
    after
}
fn verify_trace() -> Value {
    use nus_exchange_contract::s3::{journal, record::Prepared};
    TRACE.with_borrow_mut(|trace| {
        let t=trace.as_mut().unwrap();
        #[cfg(feature = "dev-local-demo")]
        {
            use nus_exchange_contract::s3::dev_local::Engine;
            let view=t.dev.engine.as_ref().unwrap().reader().get().unwrap();
            let ledger=view.receipts.values().cloned().collect::<Vec<_>>();
            drop(t.dev.engine.take());
            for _ in 0..2 {
                let reopened=Engine::open(&t.dev.home,t.dev.config.clone()).unwrap();
                let actual=reopened.reader().get().unwrap();
                assert_eq!(actual.commit,view.commit);assert_eq!(actual.state,view.state);assert_eq!(actual.receipts,view.receipts);
                reopened.reconcile_receipt_ledger(&ledger).unwrap();drop(reopened);
            }
            dev_fixture::evidence(&format!("economic-ledger-{}",view.commit.record_hash),&json!({"scope":"SYNTHETIC_RPC_REAL_DEV_STORE_COMPONENT","state":view.state,"receipt_ledger":ledger,"replay_runs":"2","expected_diff":[],"result":"PASS"}));
            dev_fixture::copy_home("economic",&t.dev.home);
            t.dev.engine=Some(Engine::open(&t.dev.home,t.dev.config.clone()).unwrap());
        }
        let mut hashes=vec![];
        for _ in 0..2 {
            let mut state=t.initial.clone();let mut commit=empty_commit();
            for (record,objects) in &t.records {
                state=Prepared::replay(&state,record,objects,&commit).unwrap().0;
                let frame=journal::frame(&canonical(record).unwrap()).unwrap();
                commit.command_seq=state.sequence();commit.record_hash=sha256(&frame);commit.end_offset+=frame.len() as u64;
            }
            assert_eq!(commit,t.commit);
            hashes.push(state.full_hash().unwrap());
        }
        assert_eq!(hashes[0],hashes[1]);
        json!({"result":"PASS","replay_hashes":hashes,"commit_hash":t.commit.record_hash,"commands":t.records.len(),"records":t.records.iter().map(|(r,_)|r).collect::<Vec<_>>()})
    })
}
fn evidence(name: &str, value: Value) {
    if let Some(dir) = std::env::var_os("S3_CANDIDATE_EVIDENCE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let raw = serde_json::to_vec_pretty(&value).unwrap();
        let path = dir.join(format!("{name}.json"));
        if path.exists() {
            assert_eq!(
                std::fs::read(&path).unwrap(),
                raw,
                "deterministic repetition"
            );
        } else {
            std::fs::write(path, raw).unwrap();
        }
    }
}

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../../protocol/s3/vectors/signed.json")).unwrap()
}
fn key(i: usize) -> Value {
    let keys: Value =
        serde_json::from_str(include_str!("../../protocol/s3/vectors/test-keys.json")).unwrap();
    keys[i].clone()
}
fn owner(i: usize) -> String {
    STANDARD.encode(hex::decode(key(i)["owner_raw_hex"].as_str().unwrap()).unwrap())
}
fn sign(i: usize, raw: &[u8]) -> Vec<u8> {
    let k = key(i);
    let seed: [u8; 32] = hex::decode(k["test_seed_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let (_, sk) = ml_dsa_65::KG::keygen_from_seed(&seed);
    sk.try_sign_with_seed(&[31; 32], raw, &[]).unwrap().to_vec()
}
fn sign_order(
    c: &Candidate,
    i: usize,
    side: &str,
    q: u64,
    p: u64,
    nonce: u8,
) -> (Vec<u8>, Vec<u8>) {
    let f = fixtures();
    let mut v = Codec::default()
        .decode(
            "OrderV1",
            &hex::decode(f["cases"][0]["canonical_hex"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
    v["owner"] = json!(owner(i));
    v["owner_pubkey"] =
        json!(STANDARD.encode(hex::decode(key(i)["public_key_hex"].as_str().unwrap()).unwrap()));
    v["genesis_hash"] = c.snapshot().context()["genesis_hash"].clone();
    v["side"] = json!(side);
    v["max_qty_lots"] = json!(q.to_string());
    v["limit_price_ticks"] = json!(p.to_string());
    v["order_id"] = json!(hex::encode([nonce; 32]));
    v["max_fee_bps"] = json!(c.snapshot().bps().to_string());
    let raw = Codec::default().encode("OrderV1", &v).unwrap();
    let sig = sign(i, &codec::frame("NUS/ORDER/V1", &raw));
    (raw, sig)
}
fn observation(c: &Candidate) -> Observation {
    Observation {
        snapshot_id: c.latest().id().into(),
        cursor_height: c.latest().height(),
        received_at: NOW,
        query_latency_ms: 1,
        catching_up: false,
    }
}
fn finish_snapshot(mut v: Value) -> Value {
    for i in 0..2 {
        let sum: u128 = v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| codec::integer(&a["assets"][i]["confirmed_atoms"], 128).unwrap())
            .sum();
        let bank: u128 = v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| codec::integer(&a["assets"][i]["bank_atoms"], 128).unwrap())
            .sum();
        let t = codec::integer(&v["assets"][i]["treasury_atoms"], 128).unwrap();
        let u = codec::integer(&v["assets"][i]["unassigned_atoms"], 128).unwrap();
        v["assets"][i]["sum_confirmed_atoms"] = json!(sum.to_string());
        v["assets"][i]["module_bank_atoms"] = json!((sum + t + u).to_string());
        v["assets"][i]["supply_atoms"] = json!((bank + sum + t + u).to_string());
    }
    v.as_object_mut().unwrap().remove("snapshot_id");
    let hash = schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", &v).unwrap();
    v["snapshot_id"] = json!(hash);
    v
}
fn setup(bps: u32, all_funded: bool) -> Candidate {
    let a: Value = serde_json::from_str(include_str!(
        "../../protocol/s3/vectors/correction-state-hash.json"
    ))
    .unwrap();
    let mut v = a["initial_state"]["chain_snapshot"].clone();
    v["height"] = json!("100");
    v["block_time_unix_ms"] = json!(NOW.to_string());
    v["terminal_batch_seqs"] = json!([]);
    v["last_batch_seq"] = json!("0");
    v["last_batch_hash"] = json!(schema::ZERO);
    v["operator"] = json!(owner(16));
    if bps == 25 {
        let f = fixtures();
        let wire = Codec::default()
            .decode(
                "OrderV1",
                &hex::decode(f["cases"][2]["canonical_hex"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
        v["context"]["genesis_hash"] = wire["genesis_hash"].clone();
    }
    for a in v["accounts"].as_array_mut().unwrap() {
        for i in 0..2 {
            a["assets"][i]["confirmed_atoms"] = json!("1000000000000");
            a["assets"][i]["bank_atoms"] = json!("0");
        }
    }
    if !all_funded {
        for a in v["accounts"].as_array_mut().unwrap() {
            if a["owner"] == owner(0) {
                a["assets"][0]["confirmed_atoms"] = json!("10000000");
                a["assets"][1]["confirmed_atoms"] = json!("0");
            }
            if a["owner"] == owner(1) {
                a["assets"][0]["confirmed_atoms"] = json!("0");
                a["assets"][1]["confirmed_atoms"] = json!("100000000");
            }
        }
    }
    #[cfg(feature = "dev-local-demo")]
    let config = {
        let inputs = dev_fixture::inputs(bps, v["accounts"].as_array().unwrap());
        let config = nus_exchange_contract::s3::dev_local::Validated::new(inputs).unwrap();
        v["context"] = config.context().clone();
        config
    };
    v = finish_snapshot(v);
    let owners = v["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| schema::bytes(&a["owner"]).unwrap())
        .collect();
    let binding = Binding::new(
        v["context"].clone(),
        owners,
        [
            codec::integer(&v["assets"][0]["supply_atoms"], 128).unwrap(),
            codec::integer(&v["assets"][1]["supply_atoms"], 128).unwrap(),
        ],
        bps,
    )
    .unwrap();
    let c = Candidate::new(binding.decode(&canonical(&v).unwrap()).unwrap()).unwrap();
    TRACE.with_borrow_mut(|t| {
        *t = Some(Trace {
            #[cfg(feature = "dev-local-demo")]
            dev: {
                let home = dev_fixture::home(bps);
                let engine = nus_exchange_contract::s3::dev_local::Engine::create(
                    &home,
                    config.clone(),
                    &canonical(&v).unwrap(),
                )
                .unwrap();
                DevTrace {
                    engine: Some(engine),
                    config,
                    home,
                }
            },
            initial: c.clone(),
            records: vec![],
            commit: empty_commit(),
        })
    });
    c
}
fn order(c: Candidate, i: usize, side: &str, q: u64, p: u64, nonce: u8) -> Candidate {
    let (raw, sig) = sign_order(&c, i, side, q, p, nonce);
    let (n, r, d) = c
        .submit("ORDER", &raw, &sig, &owner(i), &observation(&c), NOW)
        .unwrap();
    assert_eq!(r.code, "OK");
    assert!(!d);
    n.full_state().unwrap();
    replay_check(&c, n, "ORDER")
}
fn uint(tag: u64, n: u64) -> Vec<u8> {
    fn put(mut n: u64, v: &mut Vec<u8>) {
        while n >= 128 {
            v.push((n as u8) | 128);
            n >>= 7;
        }
        v.push(n as u8);
    }
    let mut v = vec![];
    put(tag << 3, &mut v);
    put(n, &mut v);
    v
}
fn blob(tag: u64, b: &[u8]) -> Vec<u8> {
    let mut key = uint(tag, 0);
    key[0] |= 2;
    key.pop();
    let len = uint(0, b.len() as u64);
    key.extend_from_slice(&len[1..]);
    key.extend_from_slice(b);
    key
}
fn join(v: &[Vec<u8>]) -> Vec<u8> {
    v.concat()
}
fn attempt(c: &mut Candidate, kind: &str, e: Option<&Value>) -> Value {
    use bech32::ToBase32;
    let b = c.batches().last().unwrap()["batch"].clone();
    let raw = c.batch_wire(b["batch_id"].as_str().unwrap()).unwrap();
    let op = bech32::encode(
        "nus",
        schema::bytes(&json!(owner(16))).unwrap().to_base32(),
        bech32::Variant::Bech32,
    )
    .unwrap();
    let mut msg = join(&[blob(1, op.as_bytes()), blob(2, raw)]);
    let kindurl = if kind == "SETTLE" {
        "/nus.exchange.s3.v1.MsgSettleBatch"
    } else {
        "/nus.exchange.s3.v1.MsgCloseBatch"
    };
    if let Some(e) = e {
        msg.extend(blob(
            3,
            &hex::decode(e["failed_tx_hash"].as_str().unwrap()).unwrap(),
        ));
        msg.extend(blob(
            4,
            &hex::decode(schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", e).unwrap()).unwrap(),
        ));
    }
    let timeout = c.latest().height() + 8;
    let body = join(&[
        blob(1, &join(&[blob(1, kindurl.as_bytes()), blob(2, &msg)])),
        uint(3, timeout),
    ]);
    let pk = hex::decode(key(16)["public_key_hex"].as_str().unwrap()).unwrap();
    let pkany = join(&[
        blob(1, b"/cosmos.crypto.mldsa65.PubKey"),
        blob(2, &blob(1, &pk)),
    ]);
    let signer = join(&[blob(1, &pkany), blob(2, &blob(1, &uint(1, 1)))]);
    let (gas, fee) = if kind == "SETTLE" {
        (10_000_000, 20_000)
    } else {
        (3_000_000, 6_000)
    };
    let auth = join(&[
        blob(1, &signer),
        blob(
            2,
            &join(&[
                blob(
                    1,
                    &join(&[blob(1, b"DEVGAS"), blob(2, fee.to_string().as_bytes())]),
                ),
                uint(2, gas),
            ]),
        ),
    ]);
    let doc = join(&[
        blob(1, &body),
        blob(2, &auth),
        blob(3, b"nus-s3-dev-1"),
        uint(4, 16),
    ]);
    let tx = join(&[blob(1, &body), blob(2, &auth), blob(3, &sign(16, &doc))]);
    let tx_ref = c
        .provide_evidence(&tx, nus_exchange_contract::s3::evidence::TX)
        .unwrap();
    json!({"context":c.latest().context(),"batch":b,"attempt_no":"1","kind":kind,"state":"PREPARED","operator":owner(16),"operator_epoch":c.latest().value()["operator_epoch"],"account_number":"16","account_sequence":"0","timeout_height":timeout.to_string(),"first_possible_height":(c.latest().height()+1).to_string(),"gas_limit":gas.to_string(),"fee_atoms":fee.to_string(),"raw_tx_ref":tx_ref,"tx_hash":sha256(&tx),"broadcast_count":"0","confirmed_tx":null,"absence_proof":null})
}
fn next_snapshot(c: &Candidate) -> Value {
    let mut v = c.latest().value().clone();
    let h = c.latest().height() + 1;
    v["height"] = json!(h.to_string());
    v["block_hash"] = json!(sha256(h.to_string().as_bytes()));
    v["owner_events"] = json!([]);
    v["terminal_batch_seqs"] = json!([]);
    v
}
fn observe(c: Candidate, v: Value) -> Candidate {
    let v = finish_snapshot(v);
    let snap = c
        .snapshot()
        .decode_related(&canonical(&v).unwrap())
        .unwrap();
    replay_check(&c, c.observe(snap).unwrap(), "SNAPSHOT")
}
fn proof(c: &mut Candidate, a: &Value, code: &str) -> Value {
    let s = c.latest().value().clone();
    let raw = c
        .evidence_bytes(&a["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX)
        .unwrap();
    let block = json!({"jsonrpc":"2.0","id":1,"result":{"block_id":{"hash":s["block_hash"].as_str().unwrap().to_uppercase()},"block":{"header":{"chain_id":"nus-s3-dev-1","height":s["height"],"last_block_id":{"hash":c.snapshot().value()["block_hash"]}},"data":{"txs":[STANDARD.encode(raw)]}}}});
    let space = if code == "0" { "" } else { "exchange_s3" };
    let results = json!({"jsonrpc":"2.0","id":1,"result":{"height":s["height"],"txs_results":[{"code":code.parse::<u32>().unwrap(),"codespace":space,"gas_wanted":a["gas_limit"],"gas_used":"12345"}]}});
    let block_ref = c
        .provide_evidence(
            &serde_json::to_vec(&block).unwrap(),
            nus_exchange_contract::s3::evidence::RPC,
        )
        .unwrap();
    let results_ref = c
        .provide_evidence(
            &serde_json::to_vec(&results).unwrap(),
            nus_exchange_contract::s3::evidence::RPC,
        )
        .unwrap();
    json!({"tx_hash":a["tx_hash"],"raw_tx_ref":a["raw_tx_ref"],"height":s["height"],"tx_index":"0","block_hash":s["block_hash"],"abci_code":code,"codespace":space,"gas_wanted":a["gas_limit"],"gas_used":"12345","raw_block_response_ref":block_ref,"raw_results_response_ref":results_ref})
}
fn receipt(c: &mut Candidate, a: &Value, failed: Option<&Value>) -> Value {
    let b = &a["batch"];
    let v = json!({"protocol_version":"2","chain_id":"nus-s3-dev-1","genesis_hash":c.latest().context()["genesis_hash"],"market_id":"DEVBASE/DEVQUOTE","batch_seq":b["batch_seq"],"batch_id":b["batch_id"],"batch_hash":b["batch_hash"],"committed_height":c.latest().height().to_string(),"tx_hash":a["tx_hash"]});
    let terminal = proof(c, a, "0");
    let resolution_ref = failed.map(|e| {
        c.provide_evidence(
            &canonical(e).unwrap(),
            nus_exchange_contract::s3::evidence::TYPED,
        )
        .unwrap()
    });
    json!({"context":c.latest().context(),"batch":b,"disposition":if failed.is_some(){"VOID"}else{"COMMITTED"},"terminal_tx":terminal,"resolution_evidence_ref":resolution_ref,"batch_receipt_v2":if failed.is_some(){Value::Null}else{json!(STANDARD.encode(Codec::default().encode("BatchReceiptV1",&v).unwrap()))},"failed_tx_hash":failed.map(|e|e["failed_tx_hash"].clone()),"resolution_evidence_hash":failed.map(|e|schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1",e).unwrap())})
}
fn terminal_snapshot(c: &Candidate, a: &Value, commit: bool) -> Value {
    let mut v = next_snapshot(c);
    v["last_batch_seq"] = a["batch"]["batch_seq"].clone();
    v["last_batch_hash"] = a["batch"]["batch_hash"].clone();
    v["terminal_batch_seqs"] = json!([a["batch"]["batch_seq"]]);
    if commit {
        let mut fees = [0, 0];
        for id in a["batch"]["fill_ids"].as_array().unwrap() {
            let f = c.ledger().fill(id.as_str().unwrap()).unwrap();
            fees[0] += f.base_fee;
            fees[1] += f.quote_fee;
            for row in v["accounts"].as_array_mut().unwrap() {
                let mut base = codec::integer(&row["assets"][0]["confirmed_atoms"], 128).unwrap();
                let mut quote = codec::integer(&row["assets"][1]["confirmed_atoms"], 128).unwrap();
                if row["owner"] == f.buyer {
                    base += f.base_net;
                    quote -= u128::from(f.quantity) * u128::from(f.price);
                }
                if row["owner"] == f.seller {
                    base -= f.sell_debit;
                    quote += f.quote_net;
                }
                row["assets"][0]["confirmed_atoms"] = json!(base.to_string());
                row["assets"][1]["confirmed_atoms"] = json!(quote.to_string());
            }
        }
        for (i, fee) in fees.iter().enumerate() {
            let old = codec::integer(&v["assets"][i]["treasury_atoms"], 128).unwrap();
            v["assets"][i]["treasury_atoms"] = json!((old + fee).to_string());
        }
    }
    v
}
#[test]
fn signed_batch_committed_same_height_price_improvement_and_idempotency() {
    for bps in [0, 25] {
        for _ in 0..3 {
            let mut c = order(
                order(setup(bps, false), 0, "2", 2000, 10000, 1),
                1,
                "1",
                1000,
                12000,
                2,
            );
            let (raw, sig) = sign_order(&c, 1, "2", 1, 10000, 3);
            let (rejected, result, _) = c
                .submit("ORDER", &raw, &sig, &owner(1), &observation(&c), NOW)
                .unwrap();
            assert_eq!(result.code, "INSUFFICIENT_AVAILABLE");
            c = replay_check(&c, rejected, "ORDER");
            let raw = canonical(&json!({"request_id":"77".repeat(32)})).unwrap();
            let (n, _, _) = c
                .local_action("WITHDRAW_PREPARE", &raw, &owner(0), &observation(&c), NOW)
                .unwrap();
            c = replay_check(&c, n, "WITHDRAW_PREPARE");
            c = replay_check(
                &c,
                c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
                "SEAL_BATCH",
            );
            assert_eq!(
                c.seal_batch("NORMAL", &observation(&c), NOW).unwrap_err(),
                "BATCH_INFLIGHT"
            );
            let a = attempt(&mut c, "SETTLE", None);
            c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
            let old = c.ledger().balance(&owner(1), Asset::Quote).unwrap().clone();
            assert_eq!((old.d, old.available().unwrap()), (12_000_000, 88_000_000));
            let mut unknown = a.clone();
            unknown["state"] = json!("SUBMISSION_UNKNOWN");
            unknown["broadcast_count"] = json!("1");
            c = replay_check(&c, c.resolve_attempt(unknown).unwrap(), "RESOLVE_ATTEMPT");
            assert_eq!(c.ledger().balance(&owner(1), Asset::Quote).unwrap(), &old);
            let v = terminal_snapshot(&c, &a, true);
            c = observe(c, v);
            assert_eq!(c.mode(), "CATCHING_UP");
            let r = receipt(&mut c, &a, None);
            c = replay_check(&c, c.record_receipt(r.clone()).unwrap(), "VOID_BATCH");
            assert_eq!(c.ledger().balance(&owner(1), Asset::Quote).unwrap(), &old);
            c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
            let hash = c.full_hash().unwrap();
            assert_eq!(c.mode(), "OPEN");
            assert_eq!(
                c.ledger()
                    .balance(&owner(1), Asset::Quote)
                    .unwrap()
                    .available()
                    .unwrap(),
                90_000_000
            );
            assert_eq!(
                c.ledger().balance(&owner(1), Asset::Base).unwrap().c,
                if bps == 0 { 1_000_000 } else { 997_500 }
            );
            assert_eq!(c.ledger().pending_fees().unwrap(), [0, 0]);
            assert_eq!(c.apply().unwrap().full_hash().unwrap(), hash);
            assert_eq!(
                c.record_receipt(r.clone()).unwrap().full_hash().unwrap(),
                hash
            );
            evidence(
                &format!("committed-{bps}"),
                json!({"scope":"SYNTHETIC_RPC_PRIVATE_CANDIDATE","attempt":a,"receipt":r,"after_state":c.full_state().unwrap(),"after_state_hash":hash,"semantic_trace":verify_trace(),"expected_available_quote":"90000000","expected_diff":[],"result":"PASS"}),
            );
            let mut bad = r;
            bad["disposition"] = json!("VOID");
            assert_eq!(c.record_receipt(bad).unwrap_err(), "RECEIPT_INCONSISTENCY");
            println!(
                "S3_CANDIDATE {}",
                json!({"case":"signed-commit","scope":"SYNTHETIC_RPC_PRIVATE_CANDIDATE","bps":bps.to_string(),"state_hash":hash,"batch":a["batch"],"tx_hash":a["tx_hash"],"height":c.snapshot().height().to_string(),"price_improvement_released":"2000000","expected_diff":[],"result":"PASS"})
            );
        }
    }
}
#[test]
fn void_closes_forward_dependencies_preserves_independent_fill_and_next_slot() {
    for _ in 0..3 {
        let mut c = order(
            order(setup(0, true), 0, "2", 2000, 10000, 11),
            1,
            "1",
            1000,
            12000,
            12,
        );
        c = replay_check(
            &c,
            c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
            "SEAL_BATCH",
        );
        let a = attempt(&mut c, "SETTLE", None);
        c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
        c = order(c, 2, "1", 1000, 12000, 13); // A's same order -> F2
        c = order(c, 3, "2", 1000, 10000, 14);
        c = order(c, 2, "1", 1000, 12000, 15); // C QUOTE -> F3
        c = order(c, 1, "2", 1000, 20000, 16);
        c = order(c, 3, "1", 1000, 20000, 17); // B BASE/D QUOTE -> independent F4
        let before = c.full_state().unwrap();
        let ids: Vec<_> = before["fills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["fill_id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(ids.len(), 4);
        let mut v = next_snapshot(&c);
        for row in v["accounts"].as_array_mut().unwrap() {
            if row["owner"] == owner(0) {
                row["epoch"] = json!("1");
            }
        }
        v["owner_events"] = json!([{"kind":"BUMP_EPOCH","owner":owner(0),"before_epoch":"0","after_epoch":"1","order_hash":null,"denom":"NONE","amount_atoms":"0","request_id":"22".repeat(32),"tx_hash":"33".repeat(32),"tx_index":"1"}]);
        c = observe(c, v);
        assert_eq!(c.apply().unwrap_err(), "UNSETTLED_HOLD");
        let mut failed = a.clone();
        failed["state"] = json!("INCLUDED_FAILURE");
        failed["confirmed_tx"] = proof(&mut c, &a, "1019");
        c = replay_check(
            &c,
            c.resolve_attempt(failed.clone()).unwrap(),
            "RESOLVE_ATTEMPT",
        );
        let e = json!({"context":c.latest().context(),"batch":a["batch"],"observed_snapshot":c.latest().value(),"settle_attempts":[failed],"batch_lookup":{"context":c.latest().context(),"observed_height":c.latest().height().to_string(),"snapshot_id":c.latest().id(),"requested_seq":a["batch"]["batch_seq"],"last_seq":"0","last_hash":schema::ZERO,"status":"NOT_FOUND_AT_HEIGHT","receipt":null},"failed_tx_hash":a["tx_hash"],"rejection_code":"EPOCH_MISMATCH"});
        let mut incomplete = e.clone();
        incomplete["settle_attempts"] = json!([]);
        assert!(c.reject_final(incomplete).is_err());
        c = replay_check(&c, c.reject_final(e.clone()).unwrap(), "VOID_BATCH");
        let close = attempt(&mut c, "CLOSE", Some(&e));
        c = replay_check(&c, c.prepare_attempt(close.clone()).unwrap(), "ATTEMPT");
        let v = terminal_snapshot(&c, &close, false);
        c = observe(c, v);
        let r = receipt(&mut c, &close, Some(&e));
        c = replay_check(&c, c.record_receipt(r.clone()).unwrap(), "VOID_BATCH");
        c = replay_check(&c, c.apply().unwrap(), "SETTLEMENT_APPLY");
        let state = c.full_state().unwrap();
        evidence(
            "directed-correction",
            json!({"scope":"SYNTHETIC_RPC_PRIVATE_CANDIDATE","before_state":before,"settle_attempt":a,"failure_evidence":e,"close_attempt":close,"receipt":r,"after_state":state,"after_state_hash":c.full_hash().unwrap(),"expected_corrected":ids[..3],"expected_surviving":[ids[3]],"expected_diff":[],"result":"PASS"}),
        );
        for f in &state["fills"].as_array().unwrap()[..3] {
            assert_eq!(f["state"], "CORRECTED");
        }
        assert_eq!(state["fills"][3]["state"], "PENDING");
        assert_eq!(
            state["corrections"][0]["corrected_fill_ids"],
            json!(ids[..3])
        );
        assert!(state["corrections"][0].get("after_state_hash").is_none());
        assert_eq!(
            state["corrections"][0]["surviving_fill_ids"],
            json!([ids[3]])
        );
        c = replay_check(
            &c,
            c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
            "SEAL_BATCH",
        );
        let b = &c.batches()[1]["batch"];
        assert_eq!(b["batch_seq"], "2");
        assert_eq!(b["previous_batch_hash"], a["batch"]["batch_hash"]);
        assert_eq!(b["fill_ids"], json!([ids[3]]));
        evidence("directed-correction-trace", verify_trace());
        println!(
            "S3_CANDIDATE {}",
            json!({"case":"directed-correction","scope":"SYNTHETIC_RPC_PRIVATE_CANDIDATE","corrected":ids[..3],"survivor":ids[3],"next_batch":b,"state_hash":c.full_hash().unwrap(),"expected_diff":[],"result":"PASS"})
        );
    }
}

#[test]
fn schema_hash_and_context_negatives() {
    let c = setup(0, false);
    let valid = c.full_state().unwrap();
    schema::validate("EngineState", &valid).unwrap();
    for version in ["s3/1", "s3/2", "s2/1", ""] {
        let mut v = valid.clone();
        v["context"]["service_schema"] = json!(version);
        assert!(schema::validate("EngineState", &v).is_err());
    }
    let mut v = c.snapshot().value().clone();
    v["accounts"][0]["assets"][0]["confirmed_atoms"] = json!("01");
    assert!(
        c.snapshot()
            .decode_related(&canonical(&v).unwrap())
            .is_err()
    );
    let f: Value = serde_json::from_str(include_str!(
        "../../protocol/s3/vectors/correction-state-hash.json"
    ))
    .unwrap();
    for step in f["steps"].as_array().unwrap() {
        let state = &step["after_state"];
        schema::validate("EngineState", state).unwrap();
        assert_eq!(
            schema::hash("NUS/S3/ENGINE_STATE/V1", state).unwrap(),
            step["after_state_hash"]
        );
        let mut bad = state.clone();
        bad["corrections"][0]["after_state_hash"] = json!(schema::ZERO);
        assert!(schema::validate("EngineState", &bad).is_err());
    }
}
#[test]
fn inclusion_tampering_and_unknown_are_rejected_without_releasing_holds() {
    let mut c = order(
        order(setup(0, false), 0, "2", 2000, 10000, 21),
        1,
        "1",
        1000,
        12000,
        22,
    );
    c = replay_check(
        &c,
        c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
        "SEAL_BATCH",
    );
    let a = attempt(&mut c, "SETTLE", None);
    for field in ["batch", "raw_tx_ref", "timeout_height", "gas_limit"] {
        let mut bad = a.clone();
        match field {
            "batch" => bad["batch"]["fill_ids"] = json!([]),
            "raw_tx_ref" => {
                bad[field] = c
                    .provide_evidence(
                        b"wrong transaction",
                        nus_exchange_contract::s3::evidence::TX,
                    )
                    .unwrap()
            }
            _ => bad[field] = json!("1"),
        };
        assert!(c.prepare_attempt(bad).is_err());
    }
    c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
    let v = terminal_snapshot(&c, &a, true);
    c = observe(c, v);
    assert_eq!(c.apply().unwrap_err(), "UNSETTLED_HOLD");
    let hash = c.full_hash().unwrap();
    let valid = receipt(&mut c, &a, None);
    for field in [
        "height",
        "tx_index",
        "block_hash",
        "abci_code",
        "raw_tx_ref",
        "raw_results_response_ref",
        "batch_receipt_v2",
    ] {
        let mut bad = valid.clone();
        match field {
            "batch_receipt_v2" => bad[field] = json!(STANDARD.encode(b"invalid receipt")),
            "raw_tx_ref" => {
                bad["terminal_tx"][field] = c
                    .provide_evidence(b"wrong tx", nus_exchange_contract::s3::evidence::TX)
                    .unwrap()
            }
            "raw_results_response_ref" => {
                bad["terminal_tx"][field] = c
                    .provide_evidence(
                        b"{\"result\":{\"height\":\"101\",\"txs_results\":[]}}",
                        nus_exchange_contract::s3::evidence::RPC,
                    )
                    .unwrap()
            }
            "block_hash" => bad["terminal_tx"][field] = json!("ff".repeat(32)),
            _ => bad["terminal_tx"][field] = json!("99"),
        }
        assert!(c.record_receipt(bad).is_err(), "{field}");
        assert_eq!(c.full_hash().unwrap(), hash);
    }
    let mut forged = a.clone();
    forged["state"] = json!("INCLUDED_FAILURE");
    assert!(c.resolve_attempt(forged).is_err());
    let mut unknown = a.clone();
    unknown["state"] = json!("SUBMISSION_UNKNOWN");
    let n = c.resolve_attempt(unknown).unwrap();
    assert_eq!(n.ledger(), c.ledger());
    assert!(n.prepare_attempt(a).is_err());
}
#[test]
fn strict_wire_matches_approved_batch_and_transaction_vectors() {
    use nus_exchange_contract::s3::wire;
    let batches: Value =
        serde_json::from_str(include_str!("../../protocol/s3/vectors/batches.json")).unwrap();
    let transactions: Value =
        serde_json::from_str(include_str!("../../protocol/s3/vectors/txs.json")).unwrap();
    for tx in transactions.as_array().unwrap() {
        let b = batches
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == tx["batch_fixture"])
            .unwrap();
        let raw = hex::decode(b["canonical_hex"].as_str().unwrap()).unwrap();
        let id = wire::identity(&raw).unwrap();
        assert_eq!(id["batch_hash"], b["batch_hash"]);
        assert_eq!(id["batch_id"], b["batch_id"]);
        let v = Codec::default().decode("BatchV1", &raw).unwrap();
        assert_eq!(wire::seal(v).unwrap().0, raw);
        let mut a = json!({"kind":"SETTLE","operator":owner(16)});
        for k in [
            "tx_hash",
            "account_sequence",
            "timeout_height",
            "gas_limit",
            "fee_atoms",
        ] {
            a[k] = tx[k].clone();
        }
        a["raw_tx"] =
            json!(STANDARD.encode(hex::decode(tx["raw_tx_hex"].as_str().unwrap()).unwrap()));
        let mut objects = nus_exchange_contract::s3::evidence::Objects::default();
        let txraw = schema::bytes(&a["raw_tx"]).unwrap();
        a.as_object_mut().unwrap().remove("raw_tx");
        a["raw_tx_ref"] = objects
            .insert(&txraw, nus_exchange_contract::s3::evidence::TX)
            .unwrap();
        wire::attempt_envelope(&a, &objects, &raw).unwrap();
    }
}
#[test]
fn eight_block_absence_proof_allows_envelope_retry_but_never_correction() {
    let mut c = order(
        order(setup(0, false), 0, "2", 2000, 10000, 41),
        1,
        "1",
        1000,
        12000,
        42,
    );
    c = replay_check(
        &c,
        c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
        "SEAL_BATCH",
    );
    let a = attempt(&mut c, "SETTLE", None);
    c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
    let mut blocks = vec![];
    let mut prev = c.latest().value()["block_hash"].clone();
    for _ in 0..9 {
        let v = next_snapshot(&c);
        c = observe(c, v);
        if c.latest().height() <= 108 {
            let s = c.latest().value().clone();
            let b = json!({"result":{"block_id":{"hash":s["block_hash"]},"block":{"header":{"chain_id":"nus-s3-dev-1","height":s["height"],"last_block_id":{"hash":prev}},"data":{"txs":null}}}});
            let r = json!({"result":{"height":s["height"],"txs_results":null}});
            let br = c
                .provide_evidence(
                    &serde_json::to_vec(&b).unwrap(),
                    nus_exchange_contract::s3::evidence::RPC,
                )
                .unwrap();
            let rr = c
                .provide_evidence(
                    &serde_json::to_vec(&r).unwrap(),
                    nus_exchange_contract::s3::evidence::RPC,
                )
                .unwrap();
            blocks.push(json!({"height":s["height"],"block_hash":s["block_hash"],"raw_block_response_ref":br,"raw_results_response_ref":rr}));
            prev = s["block_hash"].clone();
        }
    }
    let p = json!({"tx_hash":a["tx_hash"],"first_possible_height":"101","timeout_height":"108","observed_height":"109","account_sequence":"0","last_batch_seq":"0","last_batch_hash":schema::ZERO,"receipt_absent":true,"blocks":blocks,"observation_snapshot_id":c.latest().id()});
    let mut expired = a.clone();
    expired["state"] = json!("EXPIRED_ABSENT_PROVEN");
    expired["absence_proof"] = p;
    for change in ["gap", "equality", "found", "history"] {
        let mut bad = expired.clone();
        match change {
            "gap" => {
                bad["absence_proof"]["blocks"]
                    .as_array_mut()
                    .unwrap()
                    .remove(3);
            }
            "equality" => bad["absence_proof"]["observed_height"] = json!("108"),
            "found" => {
                let raw = c
                    .evidence_bytes(
                        &bad["absence_proof"]["blocks"][2]["raw_block_response_ref"],
                        nus_exchange_contract::s3::evidence::RPC,
                    )
                    .unwrap();
                let mut b: Value = serde_json::from_slice(raw).unwrap();
                b["result"]["block"]["data"]["txs"] = json!([STANDARD.encode(
                    c.evidence_bytes(&a["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX)
                        .unwrap()
                )]);
                bad["absence_proof"]["blocks"][2]["raw_block_response_ref"] = c
                    .provide_evidence(
                        &serde_json::to_vec(&b).unwrap(),
                        nus_exchange_contract::s3::evidence::RPC,
                    )
                    .unwrap();
            }
            _ => bad["absence_proof"]["blocks"][2]["block_hash"] = json!(schema::ZERO),
        }
        assert!(c.resolve_attempt(bad).is_err(), "{change}");
    }
    evidence(
        "absence-proof",
        json!({"scope":"SYNTHETIC_RPC_PRIVATE_CANDIDATE","attempt":expired,"observed_snapshot":c.latest().value(),"result":"PASS","correction_authorized":false}),
    );
    let old = c.ledger().clone();
    c = replay_check(
        &c,
        c.resolve_attempt(expired.clone()).unwrap(),
        "RESOLVE_ATTEMPT",
    );
    assert_eq!(c.ledger(), &old);
    let e = json!({"context":c.latest().context(),"batch":a["batch"],"observed_snapshot":c.latest().value(),"settle_attempts":[expired],"batch_lookup":{"context":c.latest().context(),"observed_height":"109","snapshot_id":c.latest().id(),"requested_seq":"1","last_seq":"0","last_hash":schema::ZERO,"status":"NOT_FOUND_AT_HEIGHT","receipt":null},"failed_tx_hash":a["tx_hash"],"rejection_code":"EXPIRED"});
    assert_eq!(c.reject_final(e).unwrap_err(), "FAILURE_EVIDENCE_REQUIRED");
    let mut retry = attempt(&mut c, "SETTLE", None);
    retry["attempt_no"] = json!("2");
    let n = c.prepare_attempt(retry).unwrap();
    assert_eq!(n.ledger(), &old);
    assert_eq!(n.batches().len(), 1);
}

#[test]
fn semantic_replay_rejects_resealed_ledger_fifo_result_and_refs() {
    use nus_exchange_contract::s3::record::Prepared;
    let before = setup(0, false);
    let after = order(before.clone(), 0, "2", 2000, 10000, 91);
    let p = Prepared::prepare(
        &before,
        &after,
        "ORDER",
        &observation(&after),
        NOW,
        &empty_commit(),
    )
    .unwrap();
    for field in ["ledger", "fifo", "result", "raw", "context"] {
        let mut r = p.record.clone();
        let mut state: Value =
            serde_json::from_slice(&schema::bytes(&r["state_json"]).unwrap()).unwrap();
        let mut result: Value =
            serde_json::from_slice(&schema::bytes(&r["result_json"]).unwrap()).unwrap();
        match field {
            "ledger" => {
                let row = state["accounts"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .flat_map(|a| a["ledger"].as_array_mut().unwrap())
                    .find(|r| r["R"] != "0")
                    .unwrap();
                row["R"] = json!("0");
            }
            "fifo" => state["orders"][0]["view"]["admission_seq"] = json!("99"),
            "result" => result["ledger_changes"] = json!([]),
            "raw" => r["signature"] = json!(STANDARD.encode(vec![0; 3309])),
            "context" => r["context"]["genesis_hash"] = json!("ff".repeat(32)),
            _ => unreachable!(),
        }
        let hash = schema::hash("NUS/S3/ENGINE_STATE/V1", &state).unwrap();
        r["state_json"] = json!(STANDARD.encode(canonical(&state).unwrap()));
        r["after_state_hash"] = json!(hash);
        result["after_state_hash"] = r["after_state_hash"].clone();
        r["result_json"] = json!(STANDARD.encode(canonical(&result).unwrap()));
        r["result_hash"] = json!(schema::hash("NUS/S3/COMMAND_RESULT/V1", &result).unwrap());
        assert!(
            Prepared::replay(&before, &r, &after.evidence_set().unwrap(), &empty_commit()).is_err(),
            "{field}"
        );
    }
}

#[test]
fn terminal_receipt_cannot_reverse_an_already_resolved_attempt() {
    let mut c = order(
        order(setup(0, false), 0, "2", 2000, 10000, 92),
        1,
        "1",
        1000,
        12000,
        93,
    );
    c = replay_check(
        &c,
        c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
        "SEAL_BATCH",
    );
    let a = attempt(&mut c, "SETTLE", None);
    c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
    let v = terminal_snapshot(&c, &a, true);
    c = observe(c, v);
    let receipt = receipt(&mut c, &a, None);
    let mut failure = a.clone();
    failure["state"] = json!("INCLUDED_FAILURE");
    failure["confirmed_tx"] = proof(&mut c, &a, "1019");
    let n = c.resolve_attempt(failure).unwrap();
    assert_eq!(
        n.record_receipt(receipt.clone()).unwrap_err(),
        "RECEIPT_INCONSISTENCY"
    );
    let accepted = c.record_receipt(receipt.clone()).unwrap();
    assert_eq!(accepted.attempts()[0]["state"], "INCLUDED_SUCCESS");
    assert_eq!(
        accepted.attempts()[0]["confirmed_tx"],
        receipt["terminal_tx"]
    );
    assert_eq!(accepted.ledger(), c.ledger());
}

// CTO-authored regression; source helpers above remain unchanged.
#[test]
#[cfg(feature = "dev-local-demo")]
fn cto_recovery_gate_must_reject_withdraw_prepare() {
    use nus_exchange_contract::s3::dev_local::{Command, Engine as DevEngine};
    let mut c = order(
        order(setup(0, true), 0, "2", 1000, 10000, 191),
        1,
        "1",
        1000,
        10000,
        192,
    );
    c = replay_check(
        &c,
        c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
        "SEAL_BATCH",
    );
    let a = attempt(&mut c, "SETTLE", None);
    c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
    let v = next_snapshot(&c);
    c = observe(c, v);
    let mut failed = a.clone();
    failed["state"] = json!("INCLUDED_FAILURE");
    failed["confirmed_tx"] = proof(&mut c, &a, "1030"); // ASSET_DEFICIT: unexpected, closed for review.
    c = replay_check(&c, c.resolve_attempt(failed).unwrap(), "RESOLVE_ATTEMPT");
    let rejected = c.reject_final(c.rejection_evidence().unwrap()).unwrap();
    c = replay_check(&c, rejected, "VOID_BATCH");
    assert_eq!(c.mode(), "RECOVERY_REQUIRED");
    let mut rows = Vec::new();
    TRACE.with_borrow_mut(|trace| {
        let t = trace.as_mut().unwrap();
        for restart in [false, true] {
            if restart {
                drop(t.dev.engine.take());
                t.dev.engine = Some(DevEngine::open(&t.dev.home,t.dev.config.clone()).unwrap());
            }
            let engine = t.dev.engine.as_ref().unwrap();
            let before = engine.reader().get().unwrap();
            assert_eq!(before.gate, "RECOVERY_REQUIRED");
            let raw = canonical(&json!({"request_id":if restart {"b1".repeat(32)}else{"b0".repeat(32)}})).unwrap();
            let result = engine.execute(Command::Local{kind:"WITHDRAW_PREPARE".into(),raw,session_owner:owner(2)}, &[], &observation(&c), NOW);
            let after = engine.reader().get().unwrap();
            rows.push(json!({"restart":restart,"gate_before":before.gate,"gate_after":after.gate,"seq_before":before.commit.command_seq.to_string(),"seq_after":after.commit.command_seq.to_string(),"state_changed":before.state!=after.state,"response":result.as_ref().ok(),"error":result.as_ref().err().map(|e|e.to_string())}));
        }
        dev_fixture::copy_home("cto-unexpected-final-rejection", &t.dev.home);
    });
    dev_fixture::evidence(
        "cto-recovery-gate",
        &json!({"scope":"CTO_SYNTHETIC_RPC_ACTUAL_DEV_STORE","expected":"RECOVERY_REQUIRED rejection with no publication before and after restart","observed":rows}),
    );
    println!("CTO_RECOVERY_GATE {}", json!(rows));
    assert!(
        rows.iter().all(|r| r["error"].is_string()
            && r["seq_before"] == r["seq_after"]
            && r["state_changed"] == false),
        "RECOVERY_REQUIRED permitted withdrawal preparation"
    );
}

#[test]
#[cfg(feature = "dev-local-demo")]
fn recovery_gate_preserves_store_and_signed_results_after_two_replays() {
    use nus_exchange_contract::s3::dev_local::{Command, Engine as DevEngine, Error};
    use std::{collections::BTreeMap, fs, path::Path};

    fn files(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    for bps in [0, 25] {
        let mut c = setup(bps, true);
        let (original_raw, original_sig) = sign_order(&c, 0, "2", 1000, 10000, 201);
        c = order(c, 0, "2", 1000, 10000, 201);
        let original = TRACE.with_borrow(|trace| {
            trace
                .as_ref()
                .unwrap()
                .dev
                .engine
                .as_ref()
                .unwrap()
                .reader()
                .get()
                .unwrap()
                .receipts[&1]["receipt"]
                .clone()
        });
        c = order(c, 1, "1", 1000, 10000, 202);
        c = replay_check(
            &c,
            c.seal_batch("NORMAL", &observation(&c), NOW).unwrap(),
            "SEAL_BATCH",
        );
        let a = attempt(&mut c, "SETTLE", None);
        c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
        let v = next_snapshot(&c);
        c = observe(c, v);
        assert_eq!(c.mode(), "CATCHING_UP");
        // Replayed CATCHING_UP permits reconciliation and the existing bounded
        // effect API. It is not the terminal semantic recovery gate.
        TRACE.with_borrow_mut(|trace| {
            let t = trace.as_mut().unwrap();
            drop(t.dev.engine.take());
            let e = DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap();
            assert_eq!(e.reader().get().unwrap().gate, "CATCHING_UP");
            assert_eq!(
                e.committed_attempt(a["tx_hash"].as_str().unwrap()).unwrap(),
                Some(a.clone())
            );
            assert_eq!(
                e.with_committed_attempt(a["tx_hash"].as_str().unwrap(), |_, raw| sha256(raw))
                    .unwrap(),
                Some(a["tx_hash"].as_str().unwrap().to_owned())
            );
            t.dev.engine = Some(e);
        });
        let mut failed = a.clone();
        failed["state"] = json!("INCLUDED_FAILURE");
        failed["confirmed_tx"] = proof(&mut c, &a, "1030");
        c = replay_check(
            &c,
            c.resolve_attempt(failed.clone()).unwrap(),
            "RESOLVE_ATTEMPT",
        );
        assert_eq!(c.mode(), "CATCHING_UP");
        c = replay_check(
            &c,
            c.reject_final(c.rejection_evidence().unwrap()).unwrap(),
            "VOID_BATCH",
        );
        assert_eq!(c.mode(), "RECOVERY_REQUIRED");
        let (new_raw, new_sig) = sign_order(&c, 2, "2", 1000, 10000, 203);
        let mut rows = Vec::new();
        TRACE.with_borrow_mut(|trace| {
            let t = trace.as_mut().unwrap();
            let baseline = t.dev.engine.as_ref().unwrap().reader().get().unwrap();
            assert_eq!(baseline.commit.command_seq, 7);
            let ledger = baseline.receipts.values().cloned().collect::<Vec<_>>();
            let mut stored = BTreeMap::new();
            files(&t.dev.home, &t.dev.home, &mut stored);
            for replay in 0..=2 {
                if replay > 0 {
                    drop(t.dev.engine.take());
                    t.dev.engine = Some(DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap());
                }
                let e = t.dev.engine.as_ref().unwrap();
                let mut probes = vec![
                    ("new ORDER", Command::Signed { kind: "ORDER".into(), raw: new_raw.clone(), signature: new_sig.clone(), session_owner: owner(2) }),
                    ("duplicate ORDER via execute", Command::Signed { kind: "ORDER".into(), raw: original_raw.clone(), signature: original_sig.clone(), session_owner: owner(0) }),
                    ("CANCEL gate before decoding", Command::Signed { kind: "CANCEL".into(), raw: vec![], signature: vec![], session_owner: owner(0) }),
                    ("SNAPSHOT", Command::Snapshot(canonical(c.latest().value()).unwrap())),
                    ("SEAL", Command::Seal("NORMAL".into())),
                    ("ATTEMPT", Command::Attempt(a.clone())),
                    ("RESOLVE", Command::Resolve(failed.clone())),
                    ("RECEIPT gate before validation", Command::Receipt(Value::Null)),
                    ("REJECT_FINAL", Command::RejectFinal),
                    ("APPLY", Command::Apply),
                ];
                for kind in ["WITHDRAW_PREPARE", "WITHDRAW_ABORT"] {
                    probes.push((kind, Command::Local {
                        kind: kind.into(),
                        raw: canonical(&json!({"request_id":format!("{:02x}", 210 + replay).repeat(32)})).unwrap(),
                        session_owner: owner(2),
                    }));
                }
                for (label, command) in probes {
                    let err = e.execute(command, &[], &observation(&c), NOW).unwrap_err();
                    assert!(matches!(err, Error::Recovery("RECOVERY_REQUIRED")), "{label}: {err}");
                    let after = e.reader().get().unwrap();
                    assert_eq!(after.gate, "RECOVERY_REQUIRED");
                    assert_eq!(after.commit, baseline.commit, "{label}");
                    assert_eq!(after.state, baseline.state, "{label}");
                    assert_eq!(after.receipts, baseline.receipts, "{label}");
                    let mut actual = BTreeMap::new();
                    files(&t.dev.home, &t.dev.home, &mut actual);
                    assert_eq!(actual, stored, "{label}: persisted bytes changed");
                    rows.push(json!({"replay":replay,"command":label,"error":"RECOVERY_REQUIRED","seq_before":"7","seq_after":"7","state_diff":[],"receipt_diff":[],"store_diff":[]}));
                }
                // The closed gate precedes even evidence parsing/staging.
                assert!(matches!(e.execute(Command::Apply, &[(b"invalid raw".to_vec(), "invalid/media".into())], &observation(&c), NOW), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                let mut effects = 0;
                assert!(matches!(e.with_committed_attempt(a["tx_hash"].as_str().unwrap(), |_, _| effects += 1), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                assert_eq!(effects, 0);
                assert!(matches!(e.committed_attempt(a["tx_hash"].as_str().unwrap()), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                assert!(matches!(e.trusted_recovery_history(&baseline.commit, None, 64), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                assert!(matches!(e.trusted_recovery_attempt(&baseline.commit, a["tx_hash"].as_str().unwrap()), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                assert!(matches!(e.trusted_recovery_attempt_at(&baseline.commit, 0), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                assert!(matches!(e.trusted_recovery_failure(&baseline.commit, a["batch"]["batch_id"].as_str().unwrap()), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                assert!(matches!(e.trusted_reconcile_readiness(&baseline.commit, &observation(&c), NOW), Err(Error::Recovery("RECOVERY_REQUIRED"))));
                // Authenticated historical results remain queryable without a
                // new binding, even while all execute commands are refused.
                assert_eq!(e.query_signed("ORDER", &original_raw, &original_sig, &owner(0), &observation(&c), NOW).unwrap(), Some(original.clone()));
                assert_eq!(e.query_signed("ORDER", &new_raw, &new_sig, &owner(2), &observation(&c), NOW).unwrap(), None);
                assert!(e.query_signed("ORDER", &original_raw, &original_sig, &owner(2), &observation(&c), NOW).is_err());
                let mut bad_sig = original_sig.clone();
                bad_sig[0] ^= 1;
                assert!(e.query_signed("ORDER", &original_raw, &bad_sig, &owner(0), &observation(&c), NOW).is_err());
                e.reconcile_receipt_ledger(&ledger).unwrap();
                let mut bad_ledger = ledger.clone();
                bad_ledger[0]["receipt"]["durable_ack"] = json!(true);
                assert!(e.reconcile_receipt_ledger(&bad_ledger).is_err());
                let after = e.reader().get().unwrap();
                assert_eq!(after.commit, baseline.commit);
                assert_eq!(after.state, baseline.state);
                assert_eq!(after.receipts, baseline.receipts);
                let mut actual = BTreeMap::new();
                files(&t.dev.home, &t.dev.home, &mut actual);
                assert_eq!(actual, stored);
            }
            dev_fixture::evidence(&format!("recovery-ledger-fee{bps}"), &json!({
                "scope":"SYNTHETIC_RPC_REAL_DEV_STORE_COMPONENT", "result":"PASS",
                "catching_up_replay":"RECONCILIATION_AND_EFFECT_ALLOWED",
                "state":baseline.state, "receipt_ledger":ledger,
                "replay_runs":"2", "expected_diff":[], "blocked_commands":rows,
                "effect_callback_calls":0, "signed_query_original_receipt":original,
                "signed_query_new_id":null, "authentication_rejections":2,
                "stored_files_sha256":stored.iter().map(|(name, bytes)| (name.clone(), sha256(bytes))).collect::<BTreeMap<_,_>>()
            }));
            dev_fixture::copy_home("economic-recovery", &t.dev.home);
        });
    }
}

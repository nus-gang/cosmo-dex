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
    Candidate::new(binding.decode(&canonical(&v).unwrap()).unwrap()).unwrap()
}
fn order(c: Candidate, i: usize, side: &str, q: u64, p: u64, nonce: u8) -> Candidate {
    let (raw, sig) = sign_order(&c, i, side, q, p, nonce);
    let (n, r, d) = c
        .submit("ORDER", &raw, &sig, &owner(i), &observation(&c), NOW)
        .unwrap();
    assert_eq!(r.code, "OK");
    assert!(!d);
    n.full_state().unwrap();
    n
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
fn attempt(c: &Candidate, kind: &str, e: Option<&Value>) -> Value {
    use bech32::ToBase32;
    let b = &c.batches().last().unwrap()["batch"];
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
    json!({"context":c.latest().context(),"batch":b,"attempt_no":"1","kind":kind,"state":"PREPARED","operator":owner(16),"operator_epoch":c.latest().value()["operator_epoch"],"account_number":"16","account_sequence":"0","timeout_height":timeout.to_string(),"first_possible_height":(c.latest().height()+1).to_string(),"gas_limit":gas.to_string(),"fee_atoms":fee.to_string(),"raw_tx":STANDARD.encode(&tx),"tx_hash":sha256(&tx),"broadcast_count":"0","confirmed_tx":null,"absence_proof":null})
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
    c.observe(snap).unwrap()
}
fn proof(c: &Candidate, a: &Value, code: &str) -> Value {
    let s = c.latest().value();
    let raw = schema::bytes(&a["raw_tx"]).unwrap();
    let block = json!({"jsonrpc":"2.0","id":1,"result":{"block_id":{"hash":s["block_hash"].as_str().unwrap().to_uppercase()},"block":{"header":{"chain_id":"nus-s3-dev-1","height":s["height"],"last_block_id":{"hash":c.snapshot().value()["block_hash"]}},"data":{"txs":[STANDARD.encode(&raw)]}}}});
    let space = if code == "0" { "" } else { "exchange_s3" };
    let results = json!({"jsonrpc":"2.0","id":1,"result":{"height":s["height"],"txs_results":[{"code":code.parse::<u32>().unwrap(),"codespace":space,"gas_wanted":a["gas_limit"],"gas_used":"12345"}]}});
    json!({"tx_hash":a["tx_hash"],"raw_tx":a["raw_tx"],"height":s["height"],"tx_index":"0","block_hash":s["block_hash"],"abci_code":code,"codespace":space,"gas_wanted":a["gas_limit"],"gas_used":"12345","raw_block_response":STANDARD.encode(serde_json::to_vec(&block).unwrap()),"raw_results_response":STANDARD.encode(serde_json::to_vec(&results).unwrap())})
}
fn receipt(c: &Candidate, a: &Value, failed: Option<&Value>) -> Value {
    let b = &a["batch"];
    let v = json!({"protocol_version":"2","chain_id":"nus-s3-dev-1","genesis_hash":c.latest().context()["genesis_hash"],"market_id":"DEVBASE/DEVQUOTE","batch_seq":b["batch_seq"],"batch_id":b["batch_id"],"batch_hash":b["batch_hash"],"committed_height":c.latest().height().to_string(),"tx_hash":a["tx_hash"]});
    json!({"context":c.latest().context(),"batch":b,"disposition":if failed.is_some(){"VOID"}else{"COMMITTED"},"terminal_tx":proof(c,a,"0"),"batch_receipt_v2":if failed.is_some(){Value::Null}else{json!(STANDARD.encode(Codec::default().encode("BatchReceiptV1",&v).unwrap()))},"failed_tx_hash":failed.map(|e|e["failed_tx_hash"].clone()),"resolution_evidence_hash":failed.map(|e|schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1",e).unwrap())})
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
            c = rejected;
            let (n, _) = c.prepare_withdraw(&owner(0)).unwrap();
            c = n;
            c = c.seal_batch("NORMAL", &observation(&c), NOW).unwrap();
            assert_eq!(
                c.seal_batch("NORMAL", &observation(&c), NOW).unwrap_err(),
                "BATCH_INFLIGHT"
            );
            let a = attempt(&c, "SETTLE", None);
            c = c.prepare_attempt(a.clone()).unwrap();
            let old = c.ledger().balance(&owner(1), Asset::Quote).unwrap().clone();
            assert_eq!((old.d, old.available().unwrap()), (12_000_000, 88_000_000));
            let mut unknown = a.clone();
            unknown["state"] = json!("SUBMISSION_UNKNOWN");
            unknown["broadcast_count"] = json!("1");
            c = c.resolve_attempt(unknown).unwrap();
            assert_eq!(c.ledger().balance(&owner(1), Asset::Quote).unwrap(), &old);
            let v = terminal_snapshot(&c, &a, true);
            c = observe(c, v);
            assert_eq!(c.mode(), "CATCHING_UP");
            let r = receipt(&c, &a, None);
            c = c.record_receipt(r.clone()).unwrap();
            assert_eq!(c.ledger().balance(&owner(1), Asset::Quote).unwrap(), &old);
            c = c.apply().unwrap();
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
                json!({"scope":"SYNTHETIC_RPC_PRIVATE_CANDIDATE","attempt":a,"receipt":r,"after_state":c.full_state().unwrap(),"after_state_hash":hash,"expected_available_quote":"90000000","expected_diff":[],"result":"PASS"}),
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
        c = c.seal_batch("NORMAL", &observation(&c), NOW).unwrap();
        let a = attempt(&c, "SETTLE", None);
        c = c.prepare_attempt(a.clone()).unwrap();
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
        failed["confirmed_tx"] = proof(&c, &a, "1019");
        c = c.resolve_attempt(failed.clone()).unwrap();
        let e = json!({"context":c.latest().context(),"batch":a["batch"],"observed_snapshot":c.latest().value(),"settle_attempts":[failed],"batch_lookup":{"context":c.latest().context(),"observed_height":c.latest().height().to_string(),"snapshot_id":c.latest().id(),"requested_seq":a["batch"]["batch_seq"],"last_seq":"0","last_hash":schema::ZERO,"status":"NOT_FOUND_AT_HEIGHT","receipt":null},"failed_tx_hash":a["tx_hash"],"rejection_code":"EPOCH_MISMATCH"});
        let mut incomplete = e.clone();
        incomplete["settle_attempts"] = json!([]);
        assert!(c.reject_final(incomplete).is_err());
        c = c.reject_final(e.clone()).unwrap();
        let close = attempt(&c, "CLOSE", Some(&e));
        c = c.prepare_attempt(close.clone()).unwrap();
        let v = terminal_snapshot(&c, &close, false);
        c = observe(c, v);
        let r = receipt(&c, &close, Some(&e));
        c = c.record_receipt(r.clone()).unwrap();
        c = c.apply().unwrap();
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
        c = c.seal_batch("NORMAL", &observation(&c), NOW).unwrap();
        let b = &c.batches()[1]["batch"];
        assert_eq!(b["batch_seq"], "2");
        assert_eq!(b["previous_batch_hash"], a["batch"]["batch_hash"]);
        assert_eq!(b["fill_ids"], json!([ids[3]]));
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
    for version in ["s3/1", "s2/1", ""] {
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
    c = c.seal_batch("NORMAL", &observation(&c), NOW).unwrap();
    let a = attempt(&c, "SETTLE", None);
    for field in ["batch", "raw_tx", "timeout_height", "gas_limit"] {
        let mut bad = a.clone();
        match field {
            "batch" => bad["batch"]["fill_ids"] = json!([]),
            "raw_tx" => bad[field] = json!(STANDARD.encode(b"wrong transaction")),
            _ => bad[field] = json!("1"),
        };
        assert!(c.prepare_attempt(bad).is_err());
    }
    c = c.prepare_attempt(a.clone()).unwrap();
    let v = terminal_snapshot(&c, &a, true);
    c = observe(c, v);
    assert_eq!(c.apply().unwrap_err(), "UNSETTLED_HOLD");
    let hash = c.full_hash().unwrap();
    let valid = receipt(&c, &a, None);
    for field in [
        "height",
        "tx_index",
        "block_hash",
        "abci_code",
        "raw_tx",
        "raw_results_response",
        "batch_receipt_v2",
    ] {
        let mut bad = valid.clone();
        match field {
            "batch_receipt_v2" => bad[field] = json!(STANDARD.encode(b"invalid receipt")),
            "raw_tx" => bad["terminal_tx"][field] = json!(STANDARD.encode(b"wrong tx")),
            "raw_results_response" => {
                bad["terminal_tx"][field] =
                    json!(STANDARD.encode(b"{\"result\":{\"height\":\"101\",\"txs_results\":[]}}"))
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
        wire::attempt_envelope(&a, &raw).unwrap();
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
    c = c.seal_batch("NORMAL", &observation(&c), NOW).unwrap();
    let a = attempt(&c, "SETTLE", None);
    c = c.prepare_attempt(a.clone()).unwrap();
    let mut blocks = vec![];
    let mut prev = c.latest().value()["block_hash"].clone();
    for _ in 0..9 {
        let v = next_snapshot(&c);
        c = observe(c, v);
        if c.latest().height() <= 108 {
            let s = c.latest().value();
            let b = json!({"result":{"block_id":{"hash":s["block_hash"]},"block":{"header":{"chain_id":"nus-s3-dev-1","height":s["height"],"last_block_id":{"hash":prev}},"data":{"txs":null}}}});
            let r = json!({"result":{"height":s["height"],"txs_results":null}});
            blocks.push(json!({"height":s["height"],"block_hash":s["block_hash"],"raw_block_response":STANDARD.encode(serde_json::to_vec(&b).unwrap()),"raw_results_response":STANDARD.encode(serde_json::to_vec(&r).unwrap())}));
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
                let raw = schema::bytes(&bad["absence_proof"]["blocks"][2]["raw_block_response"])
                    .unwrap();
                let mut b: Value = serde_json::from_slice(&raw).unwrap();
                b["result"]["block"]["data"]["txs"] = json!([a["raw_tx"]]);
                bad["absence_proof"]["blocks"][2]["raw_block_response"] =
                    json!(STANDARD.encode(serde_json::to_vec(&b).unwrap()));
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
    c = c.resolve_attempt(expired.clone()).unwrap();
    assert_eq!(c.ledger(), &old);
    let e = json!({"context":c.latest().context(),"batch":a["batch"],"observed_snapshot":c.latest().value(),"settle_attempts":[expired],"batch_lookup":{"context":c.latest().context(),"observed_height":"109","snapshot_id":c.latest().id(),"requested_seq":"1","last_seq":"0","last_hash":schema::ZERO,"status":"NOT_FOUND_AT_HEIGHT","receipt":null},"failed_tx_hash":a["tx_hash"],"rejection_code":"EXPIRED"});
    assert_eq!(c.reject_final(e).unwrap_err(), "FAILURE_EVIDENCE_REQUIRED");
    let mut retry = attempt(&c, "SETTLE", None);
    retry["attempt_no"] = json!("2");
    let n = c.prepare_attempt(retry).unwrap();
    assert_eq!(n.ledger(), &old);
    assert_eq!(n.batches().len(), 1);
}

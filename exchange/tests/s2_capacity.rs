mod support;
use nus_exchange_contract::s2::{
    capacity,
    journal::{Commit, Error, MAX_PAYLOAD, canonical},
    record::SnapshotRecord,
    runtime::Manifest,
    sequencer::Candidate,
    snapshot::Observation,
};
use serde_json::json;
use support::*;
#[test]
fn encoded_bound_keeps_more_than_one_thousand_fills_and_all_signed_history() {
    let s = snapshot(NOW);
    let binding = Manifest::decode(&canonical(&manifest(&s)).unwrap())
        .unwrap()
        .binding()
        .unwrap();
    let mut state = Candidate::new(binding.decode(&canonical(&s).unwrap()).unwrap()).unwrap();
    let obs = Observation {
        snapshot_id: state.snapshot().id().into(),
        cursor_height: 100,
        received_at: NOW,
        query_latency_ms: 1,
        catching_up: false,
    };
    let (raw, sig) = signed("order", &[("max_qty_lots", json!("1002"))]);
    state = state
        .submit("ORDER", &raw, &sig, &owner("order"), &obs, NOW)
        .unwrap()
        .0;
    for n in 0..1001 {
        let (raw, sig) = signed(
            "buyer-order",
            &[
                ("order_id", json!(format!("{n:064x}"))),
                ("max_qty_lots", json!("1")),
            ],
        );
        let (next, outcome, _) = state
            .submit("ORDER", &raw, &sig, &owner("buyer-order"), &obs, NOW)
            .unwrap();
        assert_eq!(outcome.code, "OK");
        state = next;
    }
    let first =
        nus_exchange_contract::s2::private_view::page(&state, &owner("order"), None, 200, 1000)
            .unwrap();
    assert_eq!(first["fills"].as_array().unwrap().len(), 1000);
    let cursor = first["next_cursor"].as_str().unwrap();
    assert_eq!(cursor.len(), 75);
    let second = nus_exchange_contract::s2::private_view::page(
        &state,
        &owner("order"),
        Some(cursor),
        200,
        1000,
    )
    .unwrap();
    assert_eq!(second["fills"].as_array().unwrap().len(), 1);
    assert_eq!(second["next_cursor"], "END");
    let before = state.state_json("OPEN").unwrap();
    let maximum = capacity::maximum_correction_payload(&state, "OPEN").unwrap();
    assert_eq!(before["orders"].as_array().unwrap().len(), 1002);
    assert_eq!(before["fills"].as_array().unwrap().len(), 1001);
    let mut next = s.clone();
    next["body"]["observed_height"] = json!("101");
    next["body"]["block_time_unix_ms"] = json!(u64::MAX.to_string());
    for a in next["body"]["accounts"].as_array_mut().unwrap() {
        a["owner_epoch"] = json!(u64::MAX.to_string());
        a["sequence"] = json!(u64::MAX.to_string());
    }
    rehash(&mut next);
    let snapshot = binding.decode(&canonical(&next).unwrap()).unwrap();
    let obs = Observation {
        snapshot_id: snapshot.id().into(),
        cursor_height: 101,
        received_at: u64::MAX,
        query_latency_ms: 2000,
        catching_up: false,
    };
    // Synthetic commit anchor is only for a serialization bound check. This is
    // not an on-disk 1002-command run and does not bypass service admission.
    let commit = Commit {
        command_seq: state.sequence(),
        record_hash: "00".repeat(32),
        end_offset: 0,
    };
    let (after, record) =
        SnapshotRecord::prepare(&state, snapshot, &obs, u64::MAX, "OPEN", &commit).unwrap();
    let record = record.unwrap();
    let actual = canonical(record.record()).unwrap().len();
    assert!(actual <= maximum, "{actual} > {maximum}");
    assert_eq!(
        record.result()["affected_order_hashes"]
            .as_array()
            .unwrap()
            .len(),
        1002
    );
    assert_eq!(
        record.result()["corrected_fill_ids"]
            .as_array()
            .unwrap()
            .len(),
        1001
    );
    let after = after.state_json("OPEN").unwrap();
    assert_eq!(after["orders"].as_array().unwrap().len(), 1002);
    assert_eq!(after["fills"].as_array().unwrap().len(), 1001);
    for a in after["accounts"].as_array().unwrap() {
        for row in a["ledger"].as_array().unwrap() {
            assert_eq!(row["D"], "0");
            assert_eq!(row["P"], "0");
        }
    }
    match capacity::check(record.record(), &state, "OPEN") {
        Ok(n) => {
            assert_eq!(n, maximum);
            assert!(maximum <= MAX_PAYLOAD);
        }
        Err(Error::ResourceLimit) => assert!(maximum > MAX_PAYLOAD || actual > MAX_PAYLOAD),
        Err(e) => panic!("{e}"),
    }
    assert_eq!(state.state_json("OPEN").unwrap(), before);
    if let Ok(path) = std::env::var("S2_CAPACITY_EVIDENCE") {
        std::fs::write(path,serde_json::to_vec_pretty(&json!({"orders":"1002","fills":"1001","actual_correction_payload":actual.to_string(),"conservative_bound":maximum.to_string(),"payload_limit":MAX_PAYLOAD.to_string(),"admissible":maximum<=MAX_PAYLOAD,"scope":"candidate encoding and preflight; not a disk throughput run"})).unwrap()).unwrap();
    }
}

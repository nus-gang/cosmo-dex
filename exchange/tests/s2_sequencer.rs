use base64::{Engine, engine::general_purpose::STANDARD};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, Signer},
};
use nus_exchange_contract::{
    codec::{self, Codec},
    s2::{
        journal::{canonical, sha256},
        ledger::Asset,
        sequencer::Candidate,
        snapshot::{Binding, Observation},
    },
};
use serde_json::{Value, json};
const NOW: u64 = 1790956680000;
fn vector(id: &str) -> Value {
    let v: Value =
        serde_json::from_str(include_str!("../../protocol/s2/vectors/signed.json")).unwrap();
    v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()
        .clone()
}
fn input(id: &str) -> (Vec<u8>, Vec<u8>, String) {
    let v = vector(id);
    (
        hex::decode(v["canonical_hex"].as_str().unwrap()).unwrap(),
        hex::decode(v["signature_hex"].as_str().unwrap()).unwrap(),
        STANDARD.encode(hex::decode(v["owner_raw_hex"].as_str().unwrap()).unwrap()),
    )
}
fn changed(id: &str, changes: &[(&str, Value)]) -> (Vec<u8>, Vec<u8>, String) {
    let (raw, _, owner) = input(id);
    let name = if id == "cancel" {
        "CancelV1"
    } else {
        "OrderV1"
    };
    let domain = if id == "cancel" {
        "NUS/CANCEL/V1"
    } else {
        "NUS/ORDER/V1"
    };
    let mut v = Codec::default().decode(name, &raw).unwrap();
    for (key, value) in changes {
        v[*key] = value.clone();
    }
    let raw = Codec::default().encode(name, &v).unwrap();
    let seed: [u8; 32] = hex::decode(vector(id)["test_seed_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let (_, sk) = ml_dsa_65::KG::keygen_from_seed(&seed);
    let sig = sk
        .try_sign_with_seed(&[77; 32], &codec::frame(domain, &raw), &[])
        .unwrap();
    (raw, sig.to_vec(), owner)
}
fn setup(height: u64, epoch: u64) -> (Candidate, Observation) {
    let mut v: Value =
        serde_json::from_str::<Value>(include_str!("../../protocol/s2/vectors/snapshot.json"))
            .unwrap()["snapshot"]
            .clone();
    v["body"]["observed_height"] = json!(height.to_string());
    for a in v["body"]["accounts"].as_array_mut().unwrap() {
        a["owner_epoch"] = json!(epoch.to_string());
    }
    v["snapshot_id"] = json!(sha256(&codec::frame(
        "NUS/S2/SNAPSHOT/V1",
        &canonical(&v["body"]).unwrap()
    )));
    let body = &v["body"];
    let b = Binding::new(
        body["context"].clone(),
        body["market"].clone(),
        [
            STANDARD
                .decode(body["accounts"][0]["owner"].as_str().unwrap())
                .unwrap(),
            STANDARD
                .decode(body["accounts"][1]["owner"].as_str().unwrap())
                .unwrap(),
        ],
        [2_000_000_000_000; 2],
    )
    .unwrap();
    let s = b.decode(&serde_json::to_vec(&v).unwrap()).unwrap();
    let observation = Observation {
        snapshot_id: s.id().into(),
        cursor_height: height,
        received_at: NOW,
        query_latency_ms: 1,
        catching_up: false,
    };
    (Candidate::new(s).unwrap(), observation)
}
#[test]
fn real_signed_partial_fill_cancel_and_duplicate_have_one_effect() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, seller) = input("order");
    let (state, sell, duplicate) = state
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW)
        .unwrap();
    assert!(!duplicate);
    assert_eq!(sell.code, "OK");
    assert_eq!(
        state.ledger().balance(&seller, Asset::Base).unwrap().r,
        2_000_000
    );
    let (buy_raw, buy_sig, buyer) = input("buyer-order");
    let (state, buy, _) = state
        .submit("ORDER", &buy_raw, &buy_sig, &buyer, &obs, NOW)
        .unwrap();
    assert_eq!(buy.code, "OK");
    assert_eq!(buy.fills.len(), 1);
    assert_eq!(
        state.ledger().balance(&seller, Asset::Base).unwrap().d,
        1_000_000
    );
    assert_eq!(
        state.ledger().balance(&seller, Asset::Quote).unwrap().p,
        10_000_000
    );
    let (retry, receipt, duplicate) = state
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW + 9000)
        .unwrap();
    assert!(duplicate);
    assert_eq!(receipt, sell);
    assert_eq!(retry.sequence(), 2);
    assert_eq!(retry.ledger(), state.ledger());
    let (raw, sig, _) = input("cancel");
    let (state, cancel, _) = state
        .submit("CANCEL", &raw, &sig, &seller, &obs, NOW + 9000)
        .unwrap();
    assert_eq!(cancel.code, "OK");
    let balance = state.ledger().balance(&seller, Asset::Base).unwrap();
    assert_eq!((balance.r, balance.d), (0, 1_000_000));
    assert_eq!(state.orders()[&sell.hash].status, "CANCELLED_OFFCHAIN");
    let (retry, again, duplicate) = state
        .submit("CANCEL", &raw, &sig, &seller, &obs, NOW)
        .unwrap();
    assert!(duplicate);
    assert_eq!(again, cancel);
    assert_eq!(retry.sequence(), 3);
    assert_eq!(
        state.evidence("CANCEL", &seller, &"44".repeat(32)),
        Some((raw.as_slice(), sig.as_slice()))
    );
}
#[test]
fn invalid_signature_and_foreign_session_cannot_read_duplicate_receipt() {
    let (state, obs) = setup(100, 0);
    let (raw, mut sig, owner) = input("order");
    let (state, _, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    sig[0] ^= 1;
    assert_eq!(
        state
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
            .unwrap_err(),
        "INVALID_SIGNATURE"
    );
    assert_eq!(
        state
            .submit("ORDER", &raw, &sig, "foreign", &obs, NOW)
            .unwrap_err(),
        "FORBIDDEN"
    );
    assert_eq!(state.sequence(), 1);
}
#[test]
fn randomized_signature_retry_preserves_original_evidence_and_body_binding() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, owner) = input("order");
    let (state, first, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    let (same_raw, other_sig, _) = changed("order", &[]);
    assert_ne!(sig, other_sig);
    let (state, again, duplicate) = state
        .submit("ORDER", &same_raw, &other_sig, &owner, &obs, NOW)
        .unwrap();
    assert!(duplicate);
    assert_eq!(first, again);
    assert_eq!(
        state.evidence("ORDER", &owner, &format!("0:{}", "33".repeat(32))),
        Some((raw.as_slice(), sig.as_slice()))
    );
    let (raw, sig, _) = changed("order", &[("max_qty_lots", json!("1999"))]);
    assert_eq!(
        state
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
            .unwrap_err(),
        "ID_CONFLICT"
    );
}
#[test]
fn deterministic_rejection_is_bound_and_candidate_does_not_change_source() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, owner) = changed("order", &[("max_qty_lots", json!("10001"))]);
    let (rejected, first, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    assert_eq!(first.code, "INSUFFICIENT_AVAILABLE");
    assert_eq!(state.sequence(), 0);
    assert_eq!(rejected.sequence(), 1);
    assert_eq!(rejected.ledger(), state.ledger());
    assert!(rejected.orders().is_empty());
    let (again, receipt, duplicate) = rejected
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW + 6000)
        .unwrap();
    assert!(duplicate);
    assert_eq!(receipt, first);
    assert_eq!(again.sequence(), 1);
}
#[test]
fn epoch_expiry_margin_and_staleness_are_enforced_for_new_effects() {
    for (height, epoch, now, code) in [
        (100, 1, NOW, "EPOCH_MISMATCH"),
        (200, 0, NOW, "EXPIRED"),
        (199, 0, NOW, "EXPIRY_MARGIN"),
        (100, 0, NOW + 5001, "STALE"),
    ] {
        let (state, obs) = setup(height, epoch);
        let (raw, sig, owner) = input("order");
        let (next, outcome, _) = state
            .submit("ORDER", &raw, &sig, &owner, &obs, now)
            .unwrap();
        assert_eq!(outcome.code, code);
        assert!(next.orders().is_empty());
        assert_eq!(next.ledger(), state.ledger());
    }
}
#[test]
fn ioc_partial_callback_fill_releases_only_remainder_and_keeps_worst_buy_debit() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, seller) = input("order");
    let (state, _, _) = state
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW)
        .unwrap();
    let (raw, sig, buyer) = changed(
        "buyer-order",
        &[
            ("order_type", json!("2")),
            ("max_qty_lots", json!("3000")),
            ("limit_price_ticks", json!("11000")),
        ],
    );
    let (state, out, _) = state
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW)
        .unwrap();
    assert_eq!(out.code, "OK");
    assert_eq!(out.fills.len(), 1);
    let b = state.ledger().balance(&buyer, Asset::Quote).unwrap();
    assert_eq!((b.r, b.d), (0, 22_000_000));
    assert_eq!(
        state.ledger().balance(&buyer, Asset::Base).unwrap().p,
        2_000_000
    );
    assert_eq!(state.orders()[&out.hash].status, "CANCELLED_OFFCHAIN");
}
#[test]
fn pending_receipts_cannot_fund_new_orders() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, owner) = input("order");
    let (state, _, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    let (raw, sig, buyer) = input("buyer-order");
    let (state, _, _) = state
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW)
        .unwrap();
    let (raw, sig, _) = changed(
        "buyer-order",
        &[("side", json!("2")), ("order_id", json!("55".repeat(32)))],
    );
    let (next, result, _) = state
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW)
        .unwrap();
    assert_eq!(result.code, "INSUFFICIENT_AVAILABLE");
    assert_eq!(next.ledger(), state.ledger());
}

#[test]
fn signed_cancel_target_nonce_and_terminal_noop_are_bound() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, owner) = input("cancel");
    let (_, missing, _) = state
        .submit("CANCEL", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    assert_eq!(missing.code, "ORDER_NOT_FOUND");
    let (raw, sig, _) = input("order");
    let (state, _, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    let (raw, sig, _) = changed("cancel", &[("order_hash", json!("11".repeat(32)))]);
    let (state, wrong, _) = state
        .submit("CANCEL", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    assert_eq!(wrong.code, "ID_CONFLICT");
    let (raw, sig, _) = input("cancel");
    assert_eq!(
        state
            .submit("CANCEL", &raw, &sig, &owner, &obs, NOW)
            .unwrap_err(),
        "ID_CONFLICT"
    );
    let (raw, sig, _) = changed("cancel", &[("cancel_nonce", json!("66".repeat(32)))]);
    let (state, first, _) = state
        .submit("CANCEL", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    assert_eq!(first.code, "OK");
    let (raw, sig, _) = changed("cancel", &[("cancel_nonce", json!("77".repeat(32)))]);
    let (next, noop, _) = state
        .submit("CANCEL", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    assert_eq!(noop.code, "OK");
    assert_eq!(next.ledger(), state.ledger());
}
#[test]
fn replay_of_recorded_signed_inputs_preserves_sequence_fill_id_and_ledger() {
    let run = || {
        let (mut state, obs) = setup(100, 0);
        let mut results = Vec::new();
        for id in ["order", "buyer-order", "cancel", "order"] {
            let (raw, sig, owner) = input(id);
            let (next, out, _) = state
                .submit(
                    if id == "cancel" { "CANCEL" } else { "ORDER" },
                    &raw,
                    &sig,
                    &owner,
                    &obs,
                    NOW,
                )
                .unwrap();
            state = next;
            results.push(out);
        }
        (state, results)
    };
    let (a, ar) = run();
    let (b, br) = run();
    assert_eq!(ar, br);
    assert_eq!(a.sequence(), 3);
    assert_eq!(a.ledger(), b.ledger());
}
#[test]
fn sequential_signed_requests_cannot_overreserve_confirmed_funds() {
    let (mut state, obs) = setup(100, 0);
    for i in 1..=6 {
        let (raw, sig, owner) = changed("order", &[("order_id", json!(format!("{i:064x}")))]);
        let (next, out, _) = state
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
            .unwrap();
        assert_eq!(
            out.code,
            if i <= 5 {
                "OK"
            } else {
                "INSUFFICIENT_AVAILABLE"
            }
        );
        state = next;
        let b = state.ledger().balance(&owner, Asset::Base).unwrap();
        assert!(b.r <= b.c);
        assert_eq!(b.d, 0);
        assert_eq!(b.p, 0);
    }
    assert_eq!(state.sequence(), 6);
    assert_eq!(state.orders().len(), 5);
}

fn next_snapshot(
    state: &Candidate,
    epoch_owner: Option<&str>,
    withdraw_base: u128,
) -> nus_exchange_contract::s2::snapshot::Snapshot {
    let mut v = state.snapshot().value().clone();
    v["body"]["observed_height"] = json!((state.snapshot().height() + 1).to_string());
    if let Some(owner) = epoch_owner {
        for a in v["body"]["accounts"].as_array_mut().unwrap() {
            if a["owner"] == owner {
                let epoch: u64 = a["owner_epoch"].as_str().unwrap().parse().unwrap();
                a["owner_epoch"] = json!((epoch + 1).to_string());
                let c: u128 = a["balances"][0]["confirmed_atoms"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap();
                let bank: u128 = a["balances"][0]["bank_atoms"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap();
                a["balances"][0]["confirmed_atoms"] = json!((c - withdraw_base).to_string());
                a["balances"][0]["bank_atoms"] = json!((bank + withdraw_base).to_string());
            }
        }
        let module: u128 = v["body"]["supplies"][0]["module_atoms"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        v["body"]["supplies"][0]["module_atoms"] = json!((module - withdraw_base).to_string());
    }
    v["snapshot_id"] = json!(sha256(&codec::frame(
        "NUS/S2/SNAPSHOT/V1",
        &canonical(&v["body"]).unwrap()
    )));
    let body = &v["body"];
    let binding = Binding::new(
        body["context"].clone(),
        body["market"].clone(),
        [
            STANDARD
                .decode(body["accounts"][0]["owner"].as_str().unwrap())
                .unwrap(),
            STANDARD
                .decode(body["accounts"][1]["owner"].as_str().unwrap())
                .unwrap(),
        ],
        [2_000_000_000_000; 2],
    )
    .unwrap();
    binding.decode(&serde_json::to_vec(&v).unwrap()).unwrap()
}
#[test]
fn epoch_correction_preserves_lifetime_fills_and_original_receipt() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, seller) = input("order");
    let (state, sell, _) = state
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW)
        .unwrap();
    let (br, bs, buyer) = input("buyer-order");
    let (state, buy, _) = state.submit("ORDER", &br, &bs, &buyer, &obs, NOW).unwrap();
    let snapshot = next_snapshot(&state, Some(&seller), 10_000_000);
    let (next, correction) = state.advance(snapshot.clone()).unwrap();
    assert_eq!(correction.affected_owners.len(), 2);
    assert_eq!(
        correction.affected_order_hashes,
        vec![sell.hash.clone(), buy.hash.clone()]
    );
    assert_eq!(correction.cancelled_order_hashes, vec![sell.hash.clone()]);
    assert_eq!(correction.corrected_fill_ids, buy.fills);
    for owner in [&seller, &buyer] {
        for asset in [Asset::Base, Asset::Quote] {
            let b = next.ledger().balance(owner, asset).unwrap();
            assert_eq!((b.r, b.d, b.p), (0, 0, 0));
        }
    }
    assert_eq!(next.ledger().balance(&seller, Asset::Base).unwrap().c, 0);
    assert_eq!(next.orders()[&sell.hash].filled, 1000);
    assert_eq!(next.orders()[&buy.hash].filled, 1000);
    assert_eq!(next.orders()[&buy.hash].status, "CORRECTED");
    assert_eq!(state.orders()[&sell.hash].status, "PARTIALLY_FILLED");
    let (_, original, duplicate) = next
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW)
        .unwrap();
    assert!(duplicate);
    assert_eq!(original, sell);
    let (again, c) = next.advance(snapshot).unwrap();
    assert_eq!(again.sequence(), next.sequence());
    assert!(c.corrected_fill_ids.is_empty());
    assert_eq!(again.ledger(), next.ledger());
    let (replayed, c2) = state.advance(next.snapshot().clone()).unwrap();
    assert_eq!(replayed.ledger(), next.ledger());
    assert_eq!(c2, correction);
}
#[test]
fn withdraw_cancels_only_open_reserve_and_retains_pending_hold() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, seller) = input("order");
    let (state, sell, _) = state
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW)
        .unwrap();
    let (br, bs, buyer) = input("buyer-order");
    let (state, _, _) = state.submit("ORDER", &br, &bs, &buyer, &obs, NOW).unwrap();
    let (next, code) = state.prepare_withdraw(&seller).unwrap();
    assert_eq!(code, "UNSETTLED_HOLD");
    assert!(next.is_frozen(&seller));
    assert_eq!(next.orders()[&sell.hash].live.remaining, 0);
    let b = next.ledger().balance(&seller, Asset::Base).unwrap();
    assert_eq!((b.r, b.d), (0, 1_000_000));
    assert_eq!(
        next.ledger().balance(&seller, Asset::Quote).unwrap().p,
        10_000_000
    );
    let (raw, sig, _) = changed("order", &[("order_id", json!("aa".repeat(32)))]);
    assert_eq!(
        next.submit("ORDER", &raw, &sig, &seller, &obs, NOW)
            .unwrap()
            .1
            .code,
        "WITHDRAW_FROZEN"
    );
    assert_eq!(
        next.abort_withdraw(&seller, &obs, NOW).unwrap_err(),
        "STALE"
    );
    let (next, _) = next.advance(next_snapshot(&next, None, 0)).unwrap();
    let obs = Observation {
        snapshot_id: next.snapshot().id().into(),
        cursor_height: 101,
        ..obs
    };
    let next = next.abort_withdraw(&seller, &obs, NOW).unwrap();
    assert!(!next.is_frozen(&seller));
    assert_eq!(next.orders()[&sell.hash].live.remaining, 0);
}
#[test]
fn empty_pending_withdraw_is_ready_and_unknown_owner_is_rejected() {
    let (state, obs) = setup(100, 0);
    assert_eq!(
        state.prepare_withdraw("foreign").unwrap_err(),
        "UNKNOWN_OWNER"
    );
    let (raw, sig, owner) = input("order");
    let (state, out, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    let (next, code) = state.prepare_withdraw(&owner).unwrap();
    assert_eq!(code, "OK");
    assert_eq!(next.orders()[&out.hash].live.remaining, 0);
    assert_eq!(next.ledger().balance(&owner, Asset::Base).unwrap().r, 0);
}
#[test]
fn continuous_snapshot_expires_orders_without_epoch_correction() {
    let (state, obs) = setup(198, 0);
    let (raw, sig, owner) = input("order");
    let (state, out, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    let (state, c) = state.advance(next_snapshot(&state, None, 0)).unwrap();
    assert!(c.affected_order_hashes.is_empty());
    assert_eq!(state.orders()[&out.hash].live.remaining, 2000);
    let (next, c) = state.advance(next_snapshot(&state, None, 0)).unwrap();
    assert!(c.corrected_fill_ids.is_empty());
    assert_eq!(next.orders()[&out.hash].status, "EXPIRED");
    assert_eq!(next.ledger().balance(&owner, Asset::Base).unwrap().r, 0);
    assert_eq!(
        next.advance(state.snapshot().clone()).unwrap_err(),
        "HEIGHT_REGRESSION"
    );
}

#[test]
fn correction_includes_counterparty_followup_and_fill_execution_order() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, seller) = input("order");
    let (state, sell, _) = state
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW)
        .unwrap();
    let (raw, sig, buyer) = input("buyer-order");
    let (state, buy1, _) = state
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW)
        .unwrap();
    let (raw, sig, _) = changed("buyer-order", &[("order_id", json!("ab".repeat(32)))]);
    let (state, buy2, _) = state
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW)
        .unwrap();
    let (raw, sig, _) = changed("buyer-order", &[("order_id", json!("ac".repeat(32)))]);
    let (state, followup, _) = state
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW)
        .unwrap();
    let (next, c) = state
        .advance(next_snapshot(&state, Some(&seller), 10_000_000))
        .unwrap();
    assert_eq!(
        c.affected_order_hashes,
        vec![sell.hash, buy1.hash, buy2.hash, followup.hash.clone()]
    );
    assert_eq!(c.cancelled_order_hashes, vec![followup.hash]);
    assert_eq!(c.corrected_fill_ids, [buy1.fills, buy2.fills].concat());
    assert_eq!(next.ledger().balance(&buyer, Asset::Quote).unwrap().r, 0);
    assert_eq!(next.ledger().balance(&buyer, Asset::Quote).unwrap().d, 0);
}
#[test]
fn disconnected_counterparty_order_retains_fifo_and_reserve() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, buyer) = input("buyer-order");
    let (state, buy, _) = state
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW)
        .unwrap();
    let (_, _, seller) = input("order");
    let (next, c) = state
        .advance(next_snapshot(&state, Some(&seller), 10_000_000))
        .unwrap();
    assert!(c.affected_order_hashes.is_empty());
    assert_eq!(next.orders()[&buy.hash].live.admission_seq, 1);
    assert_eq!(next.orders()[&buy.hash].live.remaining, 1000);
    assert_eq!(
        next.ledger().balance(&buyer, Asset::Quote),
        state.ledger().balance(&buyer, Asset::Quote)
    );
}

#[test]
fn schema_state_retains_signed_evidence_quantities_and_held_outbox() {
    let (mut state, obs) = setup(100, 0);
    let mut projections = vec![state.state_json("OPEN").unwrap()];
    for id in ["order", "buyer-order", "cancel"] {
        let (raw, sig, owner) = input(id);
        state = state
            .submit(
                if id == "cancel" { "CANCEL" } else { "ORDER" },
                &raw,
                &sig,
                &owner,
                &obs,
                NOW,
            )
            .unwrap()
            .0;
        projections.push(state.state_json("OPEN").unwrap());
    }
    let v = projections.last().unwrap();
    let sell = &v["orders"][0];
    assert_eq!(sell["view"]["revision"], "3");
    assert_eq!(sell["view"]["filled_qty_lots"], "1000");
    assert_eq!(sell["view"]["cancelled_qty_lots"], "1000");
    assert_eq!(sell["view"]["remaining_qty_lots"], "0");
    assert_eq!(sell["view"]["corrected_qty_lots"], "0");
    let (raw, sig, _) = input("order");
    assert_eq!(sell["order_wire"], STANDARD.encode(raw));
    assert_eq!(sell["signature"], STANDARD.encode(sig));
    let f = &v["fills"][0];
    assert_eq!(f["state"], "PENDING");
    assert_eq!(f["revision"], "1");
    assert_eq!(f["fee_policy_version"], "1");
    assert_eq!(f["buy_D"], "10000000");
    assert_eq!(f["buyer_P"], "1000000");
    assert_eq!(f["export_state"], "HELD_S2");
    assert_eq!(f["submission_enabled"], false);
    assert_eq!(f["maker_order_hash"], v["orders"][0]["view"]["order_hash"]);
    assert_eq!(f["taker_order_hash"], v["orders"][1]["view"]["order_hash"]);
    assert_eq!(
        state.state_hash("OPEN").unwrap(),
        sha256(&codec::frame("NUS/S2/STATE/V1", &canonical(v).unwrap()))
    );
    let seller = input("order").2;
    let (corrected, _) = state
        .advance(next_snapshot(&state, Some(&seller), 1))
        .unwrap();
    let c = corrected.state_json("OPEN").unwrap();
    assert_eq!(c["orders"][0]["view"]["revision"], "4");
    assert_eq!(c["orders"][0]["view"]["filled_qty_lots"], "1000");
    assert_eq!(c["orders"][0]["view"]["corrected_qty_lots"], "1000");
    assert_eq!(c["fills"][0]["revision"], "2");
    assert_eq!(c["fills"][0]["state"], "CORRECTED");
    assert_eq!(c["fills"][0]["snapshot_id"], f["snapshot_id"]);
    assert_eq!(c["bindings"], v["bindings"]);
    projections.push(c);
    if let Ok(path) = std::env::var("S2_STATE_PROJECTIONS") {
        std::fs::write(path, serde_json::to_vec_pretty(&projections).unwrap()).unwrap();
    }
}

#[test]
fn duplicate_and_identical_snapshot_preserve_state_hash_and_entity_revision() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, owner) = input("order");
    let state = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap()
        .0;
    let duplicate = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap()
        .0;
    assert_eq!(duplicate.state_hash("OPEN"), state.state_hash("OPEN"));
    let same = state.advance(state.snapshot().clone()).unwrap().0;
    assert_eq!(same.state_hash("OPEN"), state.state_hash("OPEN"));
    assert_ne!(state.state_hash("OPEN"), state.state_hash("STALE"));
    let (withdraw, _) = state.prepare_withdraw(&owner).unwrap();
    let v = withdraw.state_json("OPEN").unwrap();
    assert_eq!(v["orders"][0]["view"]["revision"], "2");
    assert!(
        v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["owner"] == owner && a["withdraw_frozen"] == true)
    );
    let (again, _) = withdraw.prepare_withdraw(&owner).unwrap();
    assert_eq!(again.state_json("OPEN").unwrap()["orders"], v["orders"]);
}

#[test]
fn replay_preserves_full_state_hash_including_rejection_binding() {
    let run = || {
        let (mut state, obs) = setup(100, 0);
        for id in ["order", "buyer-order", "cancel"] {
            let (raw, sig, owner) = input(id);
            state = state
                .submit(
                    if id == "cancel" { "CANCEL" } else { "ORDER" },
                    &raw,
                    &sig,
                    &owner,
                    &obs,
                    NOW,
                )
                .unwrap()
                .0;
        }
        let (raw, sig, owner) = changed(
            "order",
            &[
                ("order_id", json!("ee".repeat(32))),
                ("max_qty_lots", json!("1000000")),
            ],
        );
        let (state, out, _) = state
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
            .unwrap();
        assert_eq!(out.code, "INSUFFICIENT_AVAILABLE");
        let v = state.state_json("OPEN").unwrap();
        assert_eq!(v["orders"].as_array().unwrap().len(), 2);
        assert_eq!(v["bindings"].as_array().unwrap().len(), 4);
        state.state_hash("OPEN").unwrap()
    };
    assert_eq!(run(), run());
}

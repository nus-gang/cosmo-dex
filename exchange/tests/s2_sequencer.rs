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

#[test]
fn signed_records_fsync_reopen_and_deterministically_replay_full_results() {
    use nus_exchange_contract::s2::{
        journal::{self, Commit, Journal},
        record::SignedRecord,
    };
    let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("s2-record-{}", std::process::id()));
    let (initial, obs) = setup(100, 0);
    let context = initial.snapshot().value()["body"]["context"].clone();
    let mut journal = Journal::create(&dir, context.clone()).unwrap();
    let mut state = initial.clone();
    let mut receipts = vec![];
    let mut schemas = vec![];
    let mut commits = vec![journal.commit().clone()];
    let commands = vec![
        ("ORDER", input("order")),
        ("ORDER", input("buyer-order")),
        (
            "ORDER",
            changed(
                "buyer-order",
                &[
                    ("order_id", json!("fe".repeat(32))),
                    ("max_qty_lots", json!("1000000")),
                ],
            ),
        ),
        ("CANCEL", input("cancel")),
    ];
    for (kind, (raw, sig, owner)) in &commands {
        let (next, outcome, duplicate) = state.submit(kind, raw, sig, owner, &obs, NOW).unwrap();
        assert!(!duplicate);
        let prepared = SignedRecord::prepare(
            &state,
            &next,
            &outcome,
            kind,
            &obs,
            NOW,
            "OPEN",
            journal.commit(),
        )
        .unwrap();
        assert!(prepared.receipt(journal.commit()).is_err());
        // Fixture-only capacity ceiling. Production must derive an actual worst
        // correction encoding; this harness is not a service admission policy.
        let commit = journal
            .append(prepared.record(), journal::MAX_PAYLOAD, false)
            .unwrap();
        let receipt = prepared.receipt(&commit).unwrap();
        if outcome.seq == 2 {
            assert_eq!(
                prepared.result()["ledger_changes"]
                    .as_array()
                    .unwrap()
                    .len(),
                4
            );
            assert_eq!(
                prepared.result()["affected_order_hashes"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                prepared.result()["created_fill_ids"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
        if outcome.seq == 3 {
            assert_eq!(receipt["state"], "REJECTED");
            assert_eq!(receipt["code"], "INSUFFICIENT_AVAILABLE");
            assert_eq!(prepared.result()["ledger_changes"], json!([]));
        }
        let mut wrong = commit.clone();
        wrong.record_hash = "00".repeat(32);
        assert!(prepared.receipt(&wrong).is_err());
        schemas.push(
            json!({"record":prepared.record(), "result":prepared.result(), "receipt":receipt}),
        );
        receipts.push(receipt);
        commits.push(commit);
        state = next;
    }
    let final_hash = state.state_hash("OPEN").unwrap();
    drop(journal);
    let (journal, records) = Journal::open(&dir, context).unwrap();
    let mut replay = initial;
    for (i, record) in records.iter().enumerate() {
        let (next, prepared) = SignedRecord::replay(&replay, record, "OPEN", &commits[i]).unwrap();
        // Even internally well-framed JSON is not authoritative semantic state.
        for field in [
            "after_state_hash",
            "before_state_hash",
            "result_hash",
            "signature_hash",
            "state_json",
            "result_json",
            "previous_commit_hash",
        ] {
            let mut tampered = record.clone();
            tampered[field] = json!("00");
            assert!(
                SignedRecord::replay(&replay, &tampered, "OPEN", &commits[i]).is_err(),
                "{field}"
            );
        }
        let mut tampered = record.clone();
        tampered["unexpected"] = json!(true);
        assert!(SignedRecord::replay(&replay, &tampered, "OPEN", &commits[i]).is_err());
        assert!(SignedRecord::replay(&next, record, "OPEN", &commits[i]).is_err());
        assert_eq!(prepared.record(), record);
        assert_eq!(prepared.receipt(&commits[i + 1]).unwrap(), receipts[i]);
        replay = next;
    }
    assert_eq!(replay.state_hash("OPEN").unwrap(), final_hash);
    let (raw, sig, owner) = input("order");
    let (same, outcome, duplicate) = replay
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW + 9000)
        .unwrap();
    assert!(duplicate);
    assert_eq!(receipts[outcome.seq as usize - 1]["command_seq"], "1");
    assert!(
        SignedRecord::prepare(
            &replay,
            &same,
            &outcome,
            "ORDER",
            &obs,
            NOW,
            "OPEN",
            journal.commit()
        )
        .is_err()
    );
    // Original receipt remains successful although its latest view is cancelled.
    assert_eq!(receipts[0]["state"], "LOCAL_ACCEPTED");
    assert_eq!(replay.orders()[&outcome.hash].status, "CANCELLED_OFFCHAIN");
    if let Ok(path) = std::env::var("S2_RECORD_SCHEMA_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&schemas).unwrap()).unwrap();
    }
    assert_eq!(
        journal.commit(),
        &Commit {
            command_seq: 4,
            ..commits[4].clone()
        }
    );
    drop(journal);
    let recovered =
        nus_exchange_contract::s2::recovery::SignedRecovery::open(&dir, setup(100, 0).0, "OPEN")
            .unwrap();
    assert_eq!(recovered.state().state_hash("OPEN").unwrap(), final_hash);
    assert_eq!(recovered.commit(), &commits[4]);
    for receipt in &receipts {
        let seq = receipt["command_seq"].as_str().unwrap().parse().unwrap();
        let owner = receipt["owner"].as_str().unwrap();
        assert_eq!(recovered.receipt(owner, seq), Some(receipt));
        assert!(recovered.receipt("unauthorized", seq).is_none());
    }
    assert!(recovered.receipt(&owner, 999).is_none());
    assert!(matches!(
        Journal::open(
            &dir,
            recovered.state().snapshot().value()["body"]["context"].clone()
        ),
        Err(journal::Error::WriterAlreadyRunning)
    ));
    drop(recovered);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn automatic_signed_recovery_preserves_semantic_corruption_and_rejects_wrong_bootstrap() {
    use nus_exchange_contract::s2::{
        journal::Journal, record::SignedRecord, recovery::SignedRecovery,
    };
    let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let (initial, obs) = setup(100, 0);
    let (raw, sig, owner) = input("order");
    let (next, outcome, _) = initial
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    for tamper in [false, true] {
        let dir = root.join(format!("s2-auto-recovery-{}-{tamper}", std::process::id()));
        let mut journal =
            Journal::create(&dir, initial.snapshot().value()["body"]["context"].clone()).unwrap();
        let prepared = SignedRecord::prepare(
            &initial,
            &next,
            &outcome,
            "ORDER",
            &obs,
            NOW,
            "OPEN",
            journal.commit(),
        )
        .unwrap();
        let mut record = prepared.record().clone();
        if tamper {
            record["after_state_hash"] = json!("00".repeat(32));
        }
        journal.append(&record, 16_777_216, false).unwrap();
        drop(journal);
        let wal_before = std::fs::read(dir.join("journal.wal")).unwrap();
        let marker_before = std::fs::read(dir.join("commit.marker")).unwrap();
        let bootstrap = if tamper {
            initial.clone()
        } else {
            setup(101, 0).0
        };
        assert!(SignedRecovery::open(&dir, bootstrap, "OPEN").is_err());
        assert_eq!(std::fs::read(dir.join("journal.wal")).unwrap(), wal_before);
        assert_eq!(
            std::fs::read(dir.join("commit.marker")).unwrap(),
            marker_before
        );
        let evidence: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("evidence-")
            })
            .collect();
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            std::fs::read(evidence[0].join("journal.wal")).unwrap(),
            wal_before
        );
        // Failure released the writer lock and did not silently repair anything.
        if !tamper {
            assert!(SignedRecovery::open(&dir, initial.clone(), "OPEN").is_ok());
        } else {
            assert!(SignedRecovery::open(&dir, initial.clone(), "OPEN").is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn signed_commit_boundary_publishes_after_fsync_and_retries_original_receipt() {
    use nus_exchange_contract::s2::{
        journal::{Journal, MAX_PAYLOAD},
        recovery::SignedRecovery,
    };
    let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("s2-commit-boundary-{}", std::process::id()));
    let (initial, obs) = setup(100, 0);
    drop(Journal::create(&dir, initial.snapshot().value()["body"]["context"].clone()).unwrap());
    let mut engine = SignedRecovery::open(&dir, initial.clone(), "OPEN").unwrap();
    let (raw, sig, owner) = input("order");
    let before = engine.state().state_hash("OPEN").unwrap();
    // The fixture ceiling is not a derived production correction-size bound.
    assert!(
        engine
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW, MAX_PAYLOAD + 1)
            .is_err()
    );
    assert_eq!(engine.state().state_hash("OPEN").unwrap(), before);
    assert_eq!(engine.commit().command_seq, 0);
    assert!(!engine.recovery_required());
    assert!(
        engine
            .submit("ORDER", &raw, &sig, "other-owner", &obs, NOW, MAX_PAYLOAD)
            .is_err()
    );
    let original = engine
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW, MAX_PAYLOAD)
        .unwrap();
    assert_eq!(original["durability"], "LOCAL_FSYNC");
    assert_eq!(original["state"], "LOCAL_ACCEPTED");
    assert_eq!(engine.commit().command_seq, 1);
    assert_eq!(
        engine
            .state()
            .ledger()
            .balance(&owner, Asset::Base)
            .unwrap()
            .r,
        2_000_000
    );
    let (buy_raw, buy_sig, buyer) = input("buyer-order");
    engine
        .submit("ORDER", &buy_raw, &buy_sig, &buyer, &obs, NOW, MAX_PAYLOAD)
        .unwrap();
    let (cancel_raw, cancel_sig, _) = input("cancel");
    engine
        .submit(
            "CANCEL",
            &cancel_raw,
            &cancel_sig,
            &owner,
            &obs,
            NOW,
            MAX_PAYLOAD,
        )
        .unwrap();
    let hash = engine.state().state_hash("OPEN").unwrap();
    let wal = std::fs::read(dir.join("journal.wal")).unwrap();
    let marker = std::fs::read(dir.join("commit.marker")).unwrap();
    // A changed current order and stale observation cannot rewrite the receipt.
    assert_eq!(
        engine
            .submit(
                "ORDER",
                &raw,
                &sig,
                &owner,
                &obs,
                NOW + 100_000,
                MAX_PAYLOAD
            )
            .unwrap(),
        original
    );
    assert_eq!(engine.commit().command_seq, 3);
    drop(engine);
    let mut recovered = SignedRecovery::open(&dir, initial, "OPEN").unwrap();
    assert_eq!(recovered.state().state_hash("OPEN").unwrap(), hash);
    assert_eq!(
        recovered
            .submit(
                "ORDER",
                &raw,
                &sig,
                &owner,
                &obs,
                NOW + 100_000,
                MAX_PAYLOAD
            )
            .unwrap(),
        original
    );
    assert_eq!(std::fs::read(dir.join("journal.wal")).unwrap(), wal);
    assert_eq!(std::fs::read(dir.join("commit.marker")).unwrap(), marker);
    drop(recovered);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_append_keeps_candidate_private_and_closes_signed_commit_boundary() {
    use nus_exchange_contract::s2::{
        journal::{Journal, MAX_PAYLOAD},
        recovery::SignedRecovery,
    };
    let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("s2-commit-failure-{}", std::process::id()));
    let (initial, obs) = setup(100, 0);
    drop(Journal::create(&dir, initial.snapshot().value()["body"]["context"].clone()).unwrap());
    let mut engine = SignedRecovery::open(&dir, initial.clone(), "OPEN").unwrap();
    let (raw, sig, owner) = input("order");
    let before = engine.state().state_hash("OPEN").unwrap();
    // Force marker creation failure after WAL sync, using the real append path.
    std::fs::create_dir(dir.join("marker.tmp")).unwrap();
    assert!(
        engine
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW, MAX_PAYLOAD)
            .is_err()
    );
    assert!(engine.recovery_required());
    assert_eq!(engine.state().state_hash("OPEN").unwrap(), before);
    assert!(engine.receipt(&owner, 1).is_none());
    assert_eq!(engine.commit().command_seq, 0);
    let wal = std::fs::read(dir.join("journal.wal")).unwrap();
    assert!(!wal.is_empty());
    assert!(
        engine
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW, MAX_PAYLOAD)
            .is_err()
    );
    assert_eq!(std::fs::read(dir.join("journal.wal")).unwrap(), wal);
    drop(engine);
    assert!(SignedRecovery::open(&dir, initial, "OPEN").is_err());
    assert_eq!(std::fs::read(dir.join("journal.wal")).unwrap(), wal);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persisted_bootstrap_recovers_and_rejects_missing_corrupt_or_wrong_anchor() {
    use nus_exchange_contract::s2::{journal::MAX_PAYLOAD, recovery::SignedRecovery};
    let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let (initial, obs) = setup(100, 0);
    let body = &initial.snapshot().value()["body"];
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
    let anchor = initial.snapshot().id();
    for fault in ["none", "missing", "corrupt", "anchor", "oversize"] {
        let dir = root.join(format!("s2-bootstrap-{}-{fault}", std::process::id()));
        let mut engine = SignedRecovery::create(&dir, initial.clone(), "OPEN").unwrap();
        assert!(SignedRecovery::create(&dir, initial.clone(), "OPEN").is_err());
        assert!(SignedRecovery::open_persisted(&dir, &binding, anchor, "OPEN").is_err());
        let (raw, sig, owner) = input("order");
        let receipt = engine
            .submit("ORDER", &raw, &sig, &owner, &obs, NOW, MAX_PAYLOAD)
            .unwrap();
        let hash = engine.state().state_hash("OPEN").unwrap();
        drop(engine);
        match fault {
            "missing" => std::fs::remove_file(dir.join("bootstrap.json")).unwrap(),
            "corrupt" => std::fs::write(dir.join("bootstrap.json"), b"{}").unwrap(),
            "oversize" => std::fs::write(dir.join("bootstrap.json"), vec![b' '; 65537]).unwrap(),
            _ => (),
        }
        let wal = std::fs::read(dir.join("journal.wal")).unwrap();
        let marker = std::fs::read(dir.join("commit.marker")).unwrap();
        let wrong_anchor = "0".repeat(64);
        let result = SignedRecovery::open_persisted(
            &dir,
            &binding,
            if fault == "anchor" {
                &wrong_anchor
            } else {
                anchor
            },
            "OPEN",
        );
        if fault == "none" {
            let recovered = result.unwrap();
            assert_eq!(recovered.state().state_hash("OPEN").unwrap(), hash);
            assert_eq!(recovered.receipt(&owner, 1), Some(&receipt));
            drop(recovered);
        } else {
            assert!(result.is_err());
            assert!(std::fs::read_dir(&dir).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("evidence-")
            }));
        }
        assert_eq!(std::fs::read(dir.join("journal.wal")).unwrap(), wal);
        assert_eq!(std::fs::read(dir.join("commit.marker")).unwrap(), marker);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn snapshot_correction_commits_replays_and_preserves_receipts() {
    use nus_exchange_contract::s2::{
        journal::{Commit, MAX_PAYLOAD},
        record::SnapshotRecord,
        recovery::SignedRecovery,
    };
    let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("s2-snapshot-commit-{}", std::process::id()));
    let (initial, obs) = setup(100, 0);
    let mut engine = SignedRecovery::create(&dir, initial.clone(), "OPEN").unwrap();
    let (raw, sig, seller) = input("order");
    let receipt = engine
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW, MAX_PAYLOAD)
        .unwrap();
    let (raw, sig, buyer) = input("buyer-order");
    engine
        .submit("ORDER", &raw, &sig, &buyer, &obs, NOW, MAX_PAYLOAD)
        .unwrap();
    let before = engine.state().clone();
    let previous = engine.commit().clone();
    let next = next_snapshot(&before, Some(&seller), 10_000_000);
    let observation = Observation {
        snapshot_id: next.id().into(),
        cursor_height: next.height(),
        ..obs.clone()
    };
    let (expected, prepared) =
        SnapshotRecord::prepare(&before, next.clone(), &observation, NOW, "OPEN", &previous)
            .unwrap();
    let prepared = prepared.unwrap();
    assert_eq!(prepared.record()["command_kind"], "CORRECTION");
    assert_eq!(
        prepared.result()["corrected_fill_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for field in [
        "signature",
        "external_event_ids",
        "result_hash",
        "state_json",
        "extra",
    ] {
        let mut bad = prepared.record().clone();
        bad[field] = json!("tampered");
        assert!(
            SnapshotRecord::replay(&before, &bad, "OPEN", &previous).is_err(),
            "{field}"
        );
    }
    assert!(
        SnapshotRecord::replay(
            &before,
            prepared.record(),
            "OPEN",
            &Commit {
                command_seq: 0,
                record_hash: "0".repeat(64),
                end_offset: 0
            }
        )
        .is_err()
    );
    assert_eq!(
        engine
            .advance(next.clone(), &observation, NOW, MAX_PAYLOAD)
            .unwrap(),
        Some(prepared.result().clone())
    );
    assert_eq!(
        engine.state().state_hash("OPEN").unwrap(),
        expected.state_hash("OPEN").unwrap()
    );
    assert_eq!(
        engine
            .state()
            .ledger()
            .balance(&seller, Asset::Base)
            .unwrap()
            .c,
        0
    );
    assert_eq!(
        engine
            .state()
            .ledger()
            .balance(&buyer, Asset::Base)
            .unwrap()
            .p,
        0
    );
    assert_eq!(engine.receipt(&seller, 1), Some(&receipt));
    assert!(engine.receipt(&seller, 3).is_none());
    let wal = std::fs::read(dir.join("journal.wal")).unwrap();
    assert_eq!(
        engine
            .advance(next, &observation, NOW, MAX_PAYLOAD)
            .unwrap(),
        None
    );
    assert_eq!(std::fs::read(dir.join("journal.wal")).unwrap(), wal);
    // A subsequent ordinary snapshot is a separate non-correction record.
    let next = next_snapshot(engine.state(), None, 0);
    let observation = Observation {
        snapshot_id: next.id().into(),
        cursor_height: next.height(),
        ..observation
    };
    let result = engine
        .advance(next, &observation, NOW, MAX_PAYLOAD)
        .unwrap()
        .unwrap();
    assert_eq!(result["kind"], "SNAPSHOT");
    let hash = engine.state().state_hash("OPEN").unwrap();
    drop(engine);
    let recovered = SignedRecovery::open(&dir, initial, "OPEN").unwrap();
    assert_eq!(recovered.commit().command_seq, 4);
    assert_eq!(recovered.state().state_hash("OPEN").unwrap(), hash);
    assert_eq!(recovered.receipt(&seller, 1), Some(&receipt));
    if let Ok(path) = std::env::var("S2_SNAPSHOT_RECORD_OUTPUT") {
        std::fs::write(
            path,
            serde_json::to_vec(&json!([
                {"type":"JournalRecord","value":prepared.record()},
                {"type":"CommandResult","value":prepared.result()},
                {"type":"Correction","value":prepared.correction().unwrap()}
            ]))
            .unwrap(),
        )
        .unwrap();
    }
    drop(recovered);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn correction_marker_failure_keeps_old_ledger_and_requires_recovery() {
    use nus_exchange_contract::s2::{journal::MAX_PAYLOAD, recovery::SignedRecovery};
    let root = std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("s2-correction-failure-{}", std::process::id()));
    let (initial, obs) = setup(100, 0);
    let mut engine = SignedRecovery::create(&dir, initial.clone(), "OPEN").unwrap();
    let (raw, sig, seller) = input("order");
    engine
        .submit("ORDER", &raw, &sig, &seller, &obs, NOW, MAX_PAYLOAD)
        .unwrap();
    let before = engine.state().state_hash("OPEN").unwrap();
    let next = next_snapshot(engine.state(), Some(&seller), 10_000_000);
    let observation = Observation {
        snapshot_id: next.id().into(),
        cursor_height: next.height(),
        ..obs
    };
    std::fs::create_dir(dir.join("marker.tmp")).unwrap();
    assert!(
        engine
            .advance(next.clone(), &observation, NOW, MAX_PAYLOAD)
            .is_err()
    );
    assert!(engine.recovery_required());
    assert_eq!(engine.state().state_hash("OPEN").unwrap(), before);
    let wal = std::fs::read(dir.join("journal.wal")).unwrap();
    assert!(
        engine
            .advance(next, &observation, NOW, MAX_PAYLOAD)
            .is_err()
    );
    drop(engine);
    assert!(SignedRecovery::open(&dir, initial, "OPEN").is_err());
    assert_eq!(std::fs::read(dir.join("journal.wal")).unwrap(), wal);
    std::fs::remove_dir_all(dir).unwrap();
}

fn local_input(byte: &str) -> Vec<u8> {
    canonical(&json!({"request_id":byte.repeat(32)})).unwrap()
}
#[test]
fn local_prepare_retry_preserves_sequence_evidence_and_original_epoch() {
    let (initial, obs) = setup(100, 0);
    let (raw, sig, owner) = input("order");
    let (before, order, _) = initial
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    let raw = local_input("ab");
    let (state, result, duplicate) = before
        .local_action("WITHDRAW_PREPARE", &raw, &owner, &obs, NOW)
        .unwrap();
    assert!(!duplicate);
    assert_eq!(result.code, "OK");
    assert_eq!(result.seq, 2);
    assert!(state.is_frozen(&owner));
    assert_eq!(state.orders()[&order.hash].live.remaining, 0);
    assert_eq!(state.ledger().balance(&owner, Asset::Base).unwrap().r, 0);
    let snapshot = next_snapshot(&state, Some(&owner), 0);
    let (advanced, _) = state.advance(snapshot).unwrap();
    let (retry, again, duplicate) = advanced
        .local_action("WITHDRAW_PREPARE", &raw, &owner, &obs, NOW + 9000)
        .unwrap();
    assert!(duplicate);
    assert_eq!(again, result);
    assert_eq!(
        retry.state_hash("OPEN").unwrap(),
        advanced.state_hash("OPEN").unwrap()
    );
    assert_eq!(
        retry.local_evidence("WITHDRAW_PREPARE", &owner, &"ab".repeat(32)),
        Some(raw.as_slice())
    );
    let projected = retry.state_json("OPEN").unwrap();
    let binding = projected["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "WITHDRAW_PREPARE")
        .unwrap();
    assert_eq!(binding["owner_epoch"], "0");
    assert_eq!(binding["first_command_seq"], "2");
    assert_eq!(binding["request_hash"], sha256(&raw));
}
#[test]
fn local_action_owner_and_kind_have_independent_id_namespaces() {
    let (state, obs) = setup(100, 0);
    let (_, _, owner) = input("order");
    let (_, _, other) = input("buyer-order");
    let raw = local_input("cd");
    let (state, _, _) = state
        .local_action("WITHDRAW_PREPARE", &raw, &owner, &obs, NOW)
        .unwrap();
    let (state, outcome, duplicate) = state
        .local_action("WITHDRAW_PREPARE", &raw, &other, &obs, NOW)
        .unwrap();
    assert!(!duplicate);
    assert_eq!(outcome.seq, 2);
    let (state, outcome, duplicate) = state
        .local_action("WITHDRAW_ABORT", &raw, &other, &obs, NOW)
        .unwrap();
    assert!(!duplicate);
    assert_eq!(outcome.code, "STALE");
    assert_eq!(outcome.seq, 3);
    assert!(state.is_frozen(&other));
    let (again, result, duplicate) = state
        .local_action("WITHDRAW_ABORT", &raw, &other, &obs, NOW)
        .unwrap();
    assert!(duplicate);
    assert_eq!(result, outcome);
    assert_eq!(again.sequence(), 3);
    assert_eq!(
        state
            .local_action("WITHDRAW_PREPARE", &raw, "foreign", &obs, NOW)
            .unwrap_err(),
        "ACCOUNT_KEY_UNREGISTERED"
    );
}
#[test]
fn local_action_abort_requires_new_snapshot_and_never_restores_orders() {
    let (state, obs) = setup(100, 0);
    let (raw, sig, owner) = input("order");
    let (state, order, _) = state
        .submit("ORDER", &raw, &sig, &owner, &obs, NOW)
        .unwrap();
    let (state, _, _) = state
        .local_action("WITHDRAW_PREPARE", &local_input("11"), &owner, &obs, NOW)
        .unwrap();
    let snapshot = next_snapshot(&state, None, 0);
    let fresh = Observation {
        snapshot_id: snapshot.id().into(),
        cursor_height: snapshot.height(),
        ..obs
    };
    let (state, _) = state.advance(snapshot).unwrap();
    let raw = local_input("22");
    let (after, result, duplicate) = state
        .local_action("WITHDRAW_ABORT", &raw, &owner, &fresh, NOW)
        .unwrap();
    assert!(!duplicate);
    assert_eq!(result.code, "OK");
    assert!(!after.is_frozen(&owner));
    assert_eq!(after.orders()[&order.hash].status, "CANCELLED_OFFCHAIN");
    let (reapplied, same, _) = state
        .local_action("WITHDRAW_ABORT", &raw, &owner, &fresh, NOW)
        .unwrap();
    assert_eq!(same, result);
    assert_eq!(
        reapplied.state_hash("OPEN").unwrap(),
        after.state_hash("OPEN").unwrap()
    );
    let (_, retry, duplicate) = after
        .local_action("WITHDRAW_ABORT", &raw, &owner, &fresh, NOW + 9000)
        .unwrap();
    assert!(duplicate);
    assert_eq!(retry, result);
}
#[test]
fn local_action_rejects_malformed_input_without_binding_or_effect() {
    let (state, obs) = setup(100, 0);
    let (_, _, owner) = input("order");
    let id = "ef".repeat(32);
    let invalid = [
        format!("{{\"request_id\":\"{id}\",\"request_id\":\"{id}\"}}"),
        format!("{{\"request_id\":\"{id}\",\"owner\":\"{owner}\"}}"),
        format!("{{ \"request_id\":\"{id}\"}}"),
        format!("{{\"request_id\":\"{}\"}}", "EF".repeat(32)),
        "{\"request_id\":null}".into(),
        "[]".into(),
        "{\"request_id\":\"short\"}".into(),
    ];
    let before = state.state_hash("OPEN").unwrap();
    for raw in invalid {
        assert_eq!(
            state
                .local_action("WITHDRAW_PREPARE", raw.as_bytes(), &owner, &obs, NOW)
                .unwrap_err(),
            "LOCAL_ACTION_FORMAT"
        );
        assert_eq!(state.state_hash("OPEN").unwrap(), before);
    }
    assert_eq!(
        state
            .local_action("SNAPSHOT", &local_input("ef"), &owner, &obs, NOW)
            .unwrap_err(),
        "UNSUPPORTED_VERSION"
    );
}

use nus_exchange_contract::s3::{
    dependencies::{Asset as DebitAsset, DebitDomain, Graph, Identity, State},
    ledger::{Asset, Ledger, Side},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).into()).collect()
}
fn demo(bps: u32) -> Ledger {
    let mut l = Ledger::new([("A".into(), 10_000_000, 0), ("B".into(), 0, 100_000_000)]).unwrap();
    l.reserve("sell", "A", Side::Sell, 2000, 10000).unwrap();
    l.reserve("buy", "B", Side::Buy, 1000, 12000).unwrap();
    l.match_fill("F1", "buy", "sell", 1000, 10000, bps).unwrap();
    l.cancel("sell").unwrap();
    l
}
fn amounts(bps: u32) -> BTreeMap<String, [u128; 2]> {
    BTreeMap::from([
        (
            "A".into(),
            [9_000_000, if bps == 0 { 10_000_000 } else { 9_975_000 }],
        ),
        (
            "B".into(),
            [if bps == 0 { 1_000_000 } else { 997_500 }, 90_000_000],
        ),
    ])
}
#[test]
fn same_height_candidate_releases_price_improvement_exactly_once() {
    for _ in 0..3 {
        for bps in [0, 25] {
            let mut l = demo(bps);
            let b = l.balance("B", Asset::Quote).unwrap();
            assert_eq!(
                (b.c, b.r, b.d, b.p, b.available().unwrap()),
                (100_000_000, 0, 12_000_000, 0, 88_000_000)
            );
            assert_eq!(
                l.pending_fees().unwrap(),
                if bps == 0 { [0, 0] } else { [2500, 25000] }
            );
            let before = l.clone();
            assert_eq!(
                l.reserve("reuse-P", "B", Side::Sell, 1, 10000),
                Err("INSUFFICIENT_AVAILABLE")
            );
            assert_eq!(l, before);
            l.reconcile(&amounts(bps), &ids(&["F1"]), &[], &[]).unwrap();
            for who in ["A", "B"] {
                for asset in [Asset::Base, Asset::Quote] {
                    let b = l.balance(who, asset).unwrap();
                    assert_eq!((b.r, b.d, b.p), (0, 0, 0));
                }
            }
            assert_eq!(
                l.balance("B", Asset::Quote).unwrap().available().unwrap(),
                90_000_000
            );
            assert_eq!(l.pending_fees().unwrap(), [0, 0]);
            let after = l.clone();
            l.reconcile(&amounts(bps), &ids(&["F1"]), &[], &[]).unwrap();
            assert_eq!(l, after);
            assert_eq!(
                l.reconcile(&amounts(bps), &[], &ids(&["F1"]), &[]),
                Err("TERMINAL_FILL_IMMUTABLE")
            );
            assert_eq!(l, after);
            println!(
                "S3_EVIDENCE {}",
                serde_json::json!({"case":"demo-accounting", "source":"SYNTHETIC_COMPONENT_TEST", "bps":bps.to_string(), "fill_id":"F1", "height":null, "tx_hash":null, "genesis_hash":null, "buyer_quote_before":{"C":"100000000","D":"12000000","A":"88000000"}, "expected_confirmed":amounts(bps).iter().map(|(o,c)|serde_json::json!({"owner":o,"BASE":c[0].to_string(),"QUOTE":c[1].to_string()})).collect::<Vec<_>>(), "actual_confirmed":(["A","B"].iter().map(|o|serde_json::json!({"owner":o,"BASE":l.balance(o,Asset::Base).unwrap().c.to_string(),"QUOTE":l.balance(o,Asset::Quote).unwrap().c.to_string()})).collect::<Vec<_>>()), "buyer_quote_available":l.balance("B",Asset::Quote).unwrap().available().unwrap().to_string(), "expected_diff":[], "result":"PASS"})
            );
            let q = l.quantities("sell").unwrap();
            assert_eq!(
                (
                    q.lifetime_matched,
                    q.settled,
                    q.pending,
                    q.corrected,
                    q.cancelled
                ),
                (1000, 1000, 0, 0, 1000)
            );
        }
    }
}
#[test]
fn latest_insufficient_c_keeps_entire_old_ledger_unchanged() {
    let mut l = demo(0);
    let old = l.clone();
    let c = BTreeMap::from([("A".into(), [10_000_000, 0]), ("B".into(), [0, 0])]);
    assert_eq!(
        l.reconcile(&c, &[], &[], &[]),
        Err("INSUFFICIENT_AVAILABLE")
    );
    assert_eq!(l, old);
    assert_eq!(
        l.reconcile(&BTreeMap::new(), &[], &[], &[]),
        Err("SNAPSHOT_OWNER_SET")
    );
    assert_eq!(l, old);
    let impostor = BTreeMap::from([
        ("A".into(), [10_000_000, 0]),
        ("X".into(), [0, 100_000_000]),
    ]);
    assert_eq!(
        l.reconcile(&impostor, &[], &[], &[]),
        Err("SNAPSHOT_OWNER_SET")
    );
    assert_eq!(l, old);
    l.reconcile(&c, &[], &ids(&["F1"]), &ids(&["buy", "sell"]))
        .unwrap();
    let q = l.quantities("buy").unwrap();
    assert_eq!(
        (q.lifetime_matched, q.corrected, q.remaining),
        (1000, 1000, 0)
    );
    assert_eq!(
        l.match_fill("F2", "buy", "sell", 1, 10000, 0),
        Err("CUMULATIVE_QTY_EXCEEDED")
    );
}
#[test]
fn partial_commit_preserves_other_pending_and_remaining_r() {
    let mut l = Ledger::new([("A".into(), 10000, 0), ("B".into(), 0, 10000)]).unwrap();
    l.reserve("sell", "A", Side::Sell, 10, 10).unwrap();
    l.reserve("buy", "B", Side::Buy, 10, 12).unwrap();
    l.match_fill("F1", "buy", "sell", 1, 10, 0).unwrap();
    l.match_fill("F2", "buy", "sell", 2, 10, 0).unwrap();
    l.reconcile(
        &BTreeMap::from([("A".into(), [9000, 10]), ("B".into(), [1000, 9990])]),
        &ids(&["F1"]),
        &[],
        &[],
    )
    .unwrap();
    let b = l.balance("B", Asset::Quote).unwrap();
    assert_eq!((b.r, b.d, b.available().unwrap()), (84, 24, 9882));
    let q = l.quantities("buy").unwrap();
    assert_eq!(
        (q.lifetime_matched, q.settled, q.pending, q.remaining),
        (3, 1, 2, 7)
    );
    assert_eq!(l.balance("B", Asset::Base).unwrap().p, 2000);
}
fn identity(id: &str, seq: u64, buy: &str, sell: &str, buyer: &str, seller: &str) -> Identity {
    Identity {
        fill_id: id.into(),
        command_seq: seq,
        match_index: 0,
        orders: [buy.into(), sell.into()],
        debits: [
            DebitDomain {
                owner: buyer.into(),
                epoch: 0,
                asset: DebitAsset::Quote,
            },
            DebitDomain {
                owner: seller.into(),
                epoch: 0,
                asset: DebitAsset::Base,
            },
        ],
    }
}
#[test]
fn directional_fixture_preserves_same_owner_independent_asset_and_committed_history() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../protocol/s3/vectors/correction.json")).unwrap();
    for _ in 0..3 {
        let mut g = Graph::default();
        g.append(identity("F0", 1, "old-B", "old-A", "B", "A"))
            .unwrap();
        g.committed(&ids(&["F0"])).unwrap();
        for (i, f) in fixture["fills"]
            .as_array()
            .unwrap()
            .iter()
            .take(4)
            .enumerate()
        {
            let id = f["id"].as_str().unwrap();
            let orders = if id == "F4" {
                ["D-buy", "B-sell"]
            } else if id == "F3" {
                ["C-buy2", "E-sell"]
            } else if id == "F2" {
                ["C-buy1", "A-sell"]
            } else {
                ["B-buy", "A-sell"]
            };
            let n = g
                .append(identity(
                    id,
                    i as u64 + 2,
                    orders[0],
                    orders[1],
                    f["buyer"].as_str().unwrap(),
                    f["seller"].as_str().unwrap(),
                ))
                .unwrap();
            assert_eq!(serde_json::to_value(&n.predecessors).unwrap(), f["deps"]);
        }
        g.submission_unknown(&ids(&["F1"])).unwrap();
        assert_eq!(g.node("F1").unwrap().state, State::SubmissionUnknown);
        let c = g.corrected(&ids(&["F1"])).unwrap();
        assert_eq!(
            serde_json::to_value(&c.corrected).unwrap(),
            fixture["expected_corrected"]
        );
        assert_eq!(
            serde_json::to_value(&c.surviving_pending).unwrap(),
            fixture["expected_surviving_pending"]
        );
        assert_eq!(g.node("F0").unwrap().state, State::Committed);
        let stable = g.clone();
        g.corrected(&ids(&["F1"])).unwrap();
        assert_eq!(g, stable);
        assert_eq!(g.corrected(&ids(&["F0"])), Err("COMMITTED_IMMUTABLE"));
        assert_eq!(g, stable);
        assert_eq!(g.committed(&ids(&["F2"])), Err("TERMINAL_FILL_IMMUTABLE"));
        assert_eq!(g, stable);
    }
}
#[test]
fn predecessor_replay_rejects_missing_edges_and_terminal_reversal_atomically() {
    let mut g = Graph::default();
    g.append(identity("1", 1, "b", "s", "B", "A")).unwrap();
    let old = g.clone();
    assert_eq!(
        g.replay_append(identity("2", 2, "b2", "s", "C", "A"), &[]),
        Err("DEPENDENCY_REPLAY_MISMATCH")
    );
    assert_eq!(g, old);
    g.replay_append(identity("2", 2, "b2", "s", "C", "A"), &ids(&["1"]))
        .unwrap();
    let old = g.clone();
    assert_eq!(g.committed(&ids(&["1", "missing"])), Err("FILL_NOT_FOUND"));
    assert_eq!(g, old);
    assert_eq!(g.committed(&ids(&["1", "1"])), Err("DUPLICATE_FILL"));
    assert_eq!(g, old);
    g.committed(&ids(&["1"])).unwrap();
    let n = g
        .append(identity("3", 3, "new-b", "new-s", "B", "A"))
        .unwrap();
    assert_eq!(n.predecessors, ids(&["2"]));
}
#[test]
fn corrected_domain_ends_related_remainders_without_spending_or_losing_independent_fill() {
    let c: BTreeMap<String, [u128; 2]> = ['A', 'B', 'C', 'D', 'E']
        .into_iter()
        .map(|x| (x.to_string(), [10000, 10000]))
        .collect();
    let mut l = Ledger::new(c.iter().map(|(o, c)| (o.clone(), c[0], c[1]))).unwrap();
    for (order, who, side, q) in [
        ("as", "A", Side::Sell, 5),
        ("bb", "B", Side::Buy, 1),
        ("cb1", "C", Side::Buy, 1),
        ("es", "E", Side::Sell, 1),
        ("cb2", "C", Side::Buy, 3),
        ("bs", "B", Side::Sell, 2),
        ("db", "D", Side::Buy, 2),
    ] {
        l.reserve(order, who, side, q, 10).unwrap();
    }
    for (id, b, s) in [
        ("F1", "bb", "as"),
        ("F2", "cb1", "as"),
        ("F3", "cb2", "es"),
        ("F4", "db", "bs"),
    ] {
        l.match_fill(id, b, s, 1, 10, 0).unwrap();
    }
    let independent = l.fill("F4").unwrap().clone();
    l.reconcile(
        &c,
        &[],
        &ids(&["F1", "F2", "F3"]),
        &ids(&["as", "bb", "cb1", "es", "cb2"]),
    )
    .unwrap();
    assert_eq!(l.fill("F4").unwrap(), &independent);
    let b = l.balance("B", Asset::Base).unwrap();
    assert_eq!((b.r, b.d, b.p), (1000, 1000, 0));
    assert_eq!(l.balance("D", Asset::Base).unwrap().p, 1000);
    assert_eq!(l.remaining("as"), Some(0));
    assert_eq!(l.remaining("cb2"), Some(0));
    let q = l.quantities("as").unwrap();
    assert_eq!((q.corrected, q.lifetime_matched, q.cancelled), (2, 2, 3));
}
#[test]
fn cumulative_history_1000_1001_fills_200_201_orders_fee0_fee25_not_page_limited() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../protocol/s3/vectors/correction-history.json"
    ))
    .unwrap();
    for c in fixture["cases"].as_array().unwrap() {
        let fills = c["fills"].as_array().unwrap();
        let mut counts = BTreeMap::<String, u64>::new();
        for f in fills {
            for k in ["buyer_order_hash", "seller_order_hash"] {
                *counts.entry(f[k].as_str().unwrap().into()).or_default() += 1;
            }
        }
        let seller = fills[0]["seller_order_hash"].as_str().unwrap();
        let confirmed: BTreeMap<String, [u128; 2]> =
            BTreeMap::from([("A".into(), [10_000_000, 0]), ("B".into(), [0, 10_000_000])]);
        let mut ledger =
            Ledger::new(confirmed.iter().map(|(o, c)| (o.clone(), c[0], c[1]))).unwrap();
        for (hash, q) in &counts {
            ledger
                .reserve(
                    hash,
                    if hash == seller { "A" } else { "B" },
                    if hash == seller {
                        Side::Sell
                    } else {
                        Side::Buy
                    },
                    *q,
                    401,
                )
                .unwrap();
        }
        let mut g = Graph::default();
        let bps = c["bps"].as_str().unwrap().parse().unwrap();
        for (i, f) in fills.iter().enumerate() {
            let id = f["fill_id"].as_str().unwrap();
            let b = f["buyer_order_hash"].as_str().unwrap();
            let deps: Vec<String> =
                serde_json::from_value(f["predecessor_fill_ids"].clone()).unwrap();
            g.replay_append(identity(id, i as u64 + 1, b, seller, "B", "A"), &deps)
                .unwrap();
            ledger.match_fill(id, b, seller, 1, 401, bps).unwrap();
        }
        for (owner, asset, field, kind) in [
            ("A", Asset::Base, "expected_D_BASE", 'd'),
            ("B", Asset::Quote, "expected_D_QUOTE", 'd'),
            ("B", Asset::Base, "expected_P_BASE", 'p'),
            ("A", Asset::Quote, "expected_P_QUOTE", 'p'),
        ] {
            let b = ledger.balance(owner, asset).unwrap();
            let n = if kind == 'd' { b.d } else { b.p };
            assert_eq!(n.to_string(), c[field]);
        }
        let closed = g
            .corrected(&ids(&[fills[0]["fill_id"].as_str().unwrap()]))
            .unwrap();
        assert_eq!(
            serde_json::to_value(&closed.corrected).unwrap(),
            c["expected_corrected_fill_ids"]
        );
        let expected: BTreeSet<String> =
            serde_json::from_value(c["affected_order_hashes"].clone()).unwrap();
        assert_eq!(closed.affected_orders, expected);
        ledger
            .reconcile(
                &confirmed,
                &[],
                &closed.corrected,
                &counts.keys().cloned().collect::<Vec<_>>(),
            )
            .unwrap();
        for owner in ["A", "B"] {
            for asset in [Asset::Base, Asset::Quote] {
                let b = ledger.balance(owner, asset).unwrap();
                assert_eq!((b.r, b.d, b.p), (0, 0, 0));
            }
        }
        assert_eq!(ledger.fills().len(), fills.len());
        assert_eq!(g.nodes().count(), fills.len());
        println!(
            "S3_EVIDENCE {}",
            serde_json::json!({"case":c["id"], "source":c["scope"], "fills":fills.len().to_string(), "orders":counts.len().to_string(), "corrected_count":closed.corrected.len().to_string(), "survivors":closed.surviving_pending, "expected_diff":[], "result":"PASS"})
        );
    }
}

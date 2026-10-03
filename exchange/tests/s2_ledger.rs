use nus_exchange_contract::s2::ledger::{Asset::*, Ledger, Side::*};
fn ledger() -> Ledger {
    Ledger::new([
        ("seller".into(), 10_000_000, 0),
        ("buyer".into(), 0, 100_000_000),
    ])
    .unwrap()
}
fn orders(l: &mut Ledger) {
    l.reserve("ask", "seller", Sell, 2000, 10_000).unwrap();
    l.reserve("bid", "buyer", Buy, 2000, 12_000).unwrap();
}
#[test]
fn partial_price_improvement_cancel_and_pending_not_spendable() {
    let mut l = ledger();
    orders(&mut l);
    l.match_fill("f1", "bid", "ask", 1000, 10_000, 0).unwrap();
    let b = l.balance("buyer", Quote).unwrap();
    assert_eq!(
        (b.c, b.r, b.d, b.p, b.available().unwrap()),
        (100_000_000, 12_000_000, 12_000_000, 0, 76_000_000)
    );
    assert_eq!(l.balance("buyer", Base).unwrap().p, 1_000_000);
    assert_eq!(l.balance("seller", Quote).unwrap().p, 10_000_000);
    assert_eq!(
        l.reserve("reuse", "buyer", Sell, 1, 10_000),
        Err("INSUFFICIENT_AVAILABLE")
    );
    l.cancel("ask").unwrap();
    l.cancel("bid").unwrap();
    let b = l.balance("buyer", Quote).unwrap();
    assert_eq!(
        (b.r, b.d, b.available().unwrap()),
        (0, 12_000_000, 88_000_000)
    );
    l.correct_fill("f1").unwrap();
    let corrected = l.clone();
    l.correct_fill("f1").unwrap();
    assert_eq!(l, corrected);
    assert_eq!(l.remaining("bid"), Some(0));
    assert_eq!(
        l.balance("buyer", Quote).unwrap().available().unwrap(),
        100_000_000
    );
    assert_eq!(l.balance("buyer", Base).unwrap().p, 0);
    assert!(l.reserve("bid", "buyer", Buy, 1, 1).is_err());
}
#[test]
fn all_failed_fills_leave_both_parties_unchanged() {
    let mut l = ledger();
    orders(&mut l);
    let before = l.clone();
    for (q, p, bps) in [
        (0, 10_000, 0),
        (2001, 10_000, 0),
        (1, 9999, 0),
        (1, 12001, 0),
        (1, 10_000, 10000),
        (1, 10_000, 10001),
    ] {
        assert!(l.match_fill("f", "bid", "ask", q, p, bps).is_err());
        assert_eq!(l, before);
    }
    l.match_fill("f", "bid", "ask", 1, 10_000, 0).unwrap();
    let once = l.clone();
    assert!(l.match_fill("f", "bid", "ask", 1, 10_000, 0).is_err());
    assert_eq!(l, once);
}
#[test]
fn fees_are_per_fill_ceil_and_exactly_reversed() {
    let mut l = ledger();
    orders(&mut l);
    for id in ["f1", "f2"] {
        l.match_fill(id, "bid", "ask", 1, 10_001, 25).unwrap();
    }
    let f = l.fill("f1").unwrap();
    assert_eq!((f.base_fee, f.quote_fee), (3, 26));
    assert_eq!(l.balance("buyer", Base).unwrap().p, 1994);
    assert_eq!(l.balance("seller", Quote).unwrap().p, 19950);
    l.correct_fill("f1").unwrap();
    assert_eq!(l.balance("seller", Quote).unwrap().p, 9975);
    assert_eq!(l.remaining("bid"), Some(1998));
}
#[test]
fn shared_available_prevents_overreservation_and_self_trade() {
    let mut l = Ledger::new([("a".into(), 1000, 10)]).unwrap();
    l.reserve("b", "a", Buy, 1, 10).unwrap();
    let before = l.clone();
    assert!(l.reserve("b2", "a", Buy, 1, 1).is_err());
    assert_eq!(l, before);
    l.reserve("s", "a", Sell, 1, 10).unwrap();
    let before = l.clone();
    assert_eq!(l.match_fill("f", "b", "s", 1, 10, 0), Err("SELF_TRADE"));
    assert_eq!(l, before);
}
#[test]
fn exhaustive_small_fills_conserve_net_plus_fee_and_hold_limit() {
    for q in 1..20 {
        for p in 1..20 {
            for bps in [0, 25, 5000, 10000] {
                let mut l =
                    Ledger::new([("a".into(), 100000, 0), ("b".into(), 0, 100000)]).unwrap();
                l.reserve("s", "a", Sell, q, p).unwrap();
                l.reserve("b", "b", Buy, q, p + 1).unwrap();
                let before = l.clone();
                if l.match_fill("f", "b", "s", q, p, bps).is_err() {
                    assert_eq!(l, before);
                    continue;
                }
                let f = l.fill("f").unwrap();
                assert_eq!(f.base_net + f.base_fee, u128::from(q) * 1000);
                assert_eq!(f.quote_net + f.quote_fee, u128::from(q) * u128::from(p));
                assert_eq!(f.buy_debit, u128::from(q) * u128::from(p + 1));
                l.correct_fill("f").unwrap();
                for owner in ["a", "b"] {
                    for asset in [Base, Quote] {
                        let b = l.balance(owner, asset).unwrap();
                        assert_eq!((b.r, b.d, b.p), (0, 0, 0));
                    }
                }
            }
        }
    }
}

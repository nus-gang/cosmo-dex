use nus_exchange_contract::s2::{
    ledger::{Asset, Ledger, Side},
    matching::{LiveOrder, Remainder, Tif, execute},
};
fn order(seq: u64, owner: &str, side: Side, price: u64, q: u64) -> LiveOrder {
    LiveOrder {
        hash: format!("order-{seq}"),
        owner: owner.into(),
        admission_seq: seq,
        side,
        price,
        remaining: q,
        expiry_height: 100,
    }
}
#[test]
fn price_fifo_partial_and_deterministic_rebuild() {
    let live = vec![
        order(3, "a", Side::Sell, 90, 3),
        order(1, "a", Side::Sell, 100, 2),
        order(2, "a", Side::Sell, 90, 2),
    ];
    let taker = order(4, "b", Side::Buy, 100, 4);
    let result = execute(&live, &taker, Tif::Gtc, 10, 0).unwrap();
    assert_eq!(result.remainder, Remainder::Filled);
    assert_eq!(
        result
            .fills
            .iter()
            .map(|f| (f.maker_hash.as_str(), f.quantity, f.price))
            .collect::<Vec<_>>(),
        vec![("order-2", 2, 90), ("order-3", 2, 90)]
    );
    let mut reversed = live.clone();
    reversed.reverse();
    for _ in 0..10 {
        assert_eq!(execute(&reversed, &taker, Tif::Gtc, 10, 0).unwrap(), result);
    }
    let mut next = vec![live[1].clone(), live[0].clone()];
    next[1].remaining = 1;
    let r = execute(&next, &order(5, "b", Side::Buy, 100, 2), Tif::Ioc, 10, 0).unwrap();
    assert_eq!(
        r.fills
            .iter()
            .map(|f| f.maker_hash.as_str())
            .collect::<Vec<_>>(),
        vec!["order-3", "order-1"]
    );
}
#[test]
fn sell_taker_highest_bid_first_and_price_limit() {
    let live = vec![
        order(1, "a", Side::Buy, 90, 2),
        order(2, "a", Side::Buy, 110, 2),
        order(3, "a", Side::Buy, 100, 2),
    ];
    let r = execute(&live, &order(4, "b", Side::Sell, 100, 5), Tif::Ioc, 10, 0).unwrap();
    assert_eq!(
        r.fills.iter().map(|f| f.price).collect::<Vec<_>>(),
        vec![110, 100]
    );
    assert_eq!((r.remaining, r.remainder), (1, Remainder::IocCancelled));
}
#[test]
fn ioc_error_callback_applies_once_to_ledger() {
    let maker = order(1, "a", Side::Sell, 90, 3);
    let taker = order(2, "b", Side::Buy, 100, 5);
    let r = execute(std::slice::from_ref(&maker), &taker, Tif::Ioc, 10, 0).unwrap();
    let mut ledger = Ledger::new([("a".into(), 3000, 0), ("b".into(), 0, 500)]).unwrap();
    ledger.reserve(&maker.hash, "a", Side::Sell, 3, 90).unwrap();
    ledger.reserve(&taker.hash, "b", Side::Buy, 5, 100).unwrap();
    assert_eq!(r.fills.len(), 1);
    for (i, f) in r.fills.iter().enumerate() {
        ledger
            .match_fill(
                &format!("2:{i}"),
                &taker.hash,
                &f.maker_hash,
                f.quantity,
                f.price,
                0,
            )
            .unwrap();
    }
    ledger.cancel(&taker.hash).unwrap();
    let b = ledger.balance("b", Asset::Quote).unwrap();
    assert_eq!((b.r, b.d, b.available().unwrap()), (0, 300, 200));
    assert_eq!(ledger.balance("b", Asset::Base).unwrap().p, 3000);
    assert_eq!(ledger.balance("a", Asset::Quote).unwrap().p, 270);
}
#[test]
fn stp_stops_at_self_and_preserves_prior_fill() {
    let live = vec![
        order(1, "a", Side::Sell, 90, 2),
        order(2, "b", Side::Sell, 95, 2),
        order(3, "a", Side::Sell, 100, 2),
    ];
    for tif in [Tif::Ioc, Tif::Gtc] {
        let r = execute(&live, &order(4, "b", Side::Buy, 100, 5), tif, 10, 0).unwrap();
        assert_eq!(
            (r.fills.len(), r.remaining, r.remainder),
            (1, 3, Remainder::SelfTradeCancelled)
        );
        assert_eq!(r.fills[0].maker_hash, "order-1");
    }
}
#[test]
fn fee_rejected_prefix_preserves_prior_fill() {
    let live = vec![
        order(1, "a", Side::Sell, 1, 400),
        order(2, "a", Side::Sell, 1, 1),
        order(3, "a", Side::Sell, 2, 1),
    ];
    let r = execute(&live, &order(4, "b", Side::Buy, 2, 402), Tif::Gtc, 10, 25).unwrap();
    assert_eq!(
        (r.fills.len(), r.remaining, r.remainder),
        (1, 2, Remainder::PolicyRejected)
    );
    assert_eq!(r.fills[0].quantity, 400);
}
#[test]
fn expiry_at_height_before_match_sorted_by_admission() {
    let mut a = order(1, "a", Side::Sell, 90, 2);
    a.expiry_height = 10;
    let mut b = order(2, "a", Side::Sell, 90, 2);
    b.expiry_height = 9;
    let live = vec![b, a, order(3, "a", Side::Sell, 100, 2)];
    let r = execute(&live, &order(4, "b", Side::Buy, 100, 3), Tif::Gtc, 10, 0).unwrap();
    assert_eq!(r.expired_hashes, vec!["order-1", "order-2"]);
    assert_eq!((r.remaining, r.remainder), (1, Remainder::Resting));
    assert_eq!(r.fills[0].maker_hash, "order-3");
}
#[test]
fn no_liquidity_gtc_ioc_and_invalid_inputs() {
    let taker = order(2, "b", Side::Buy, 100, 5);
    assert_eq!(
        execute(&[], &taker, Tif::Gtc, 10, 0).unwrap().remainder,
        Remainder::Resting
    );
    assert_eq!(
        execute(&[], &taker, Tif::Ioc, 10, 0).unwrap().remainder,
        Remainder::IocCancelled
    );
    assert_eq!(execute(&[], &taker, Tif::Ioc, 100, 0), Err("EXPIRED"));
    let maker = order(1, "a", Side::Sell, 90, 1);
    assert_eq!(
        execute(&[maker.clone(), maker], &taker, Tif::Ioc, 10, 0),
        Err("ADAPTER_ID_CONFLICT")
    );
    assert_eq!(
        execute(std::slice::from_ref(&taker), &taker, Tif::Gtc, 10, 0),
        Err("ADAPTER_ID_CONFLICT")
    );
    let mut invalid = taker;
    invalid.remaining = 0;
    assert_eq!(
        execute(&[], &invalid, Tif::Gtc, 10, 0),
        Err("ADAPTER_INVALID_ORDER")
    );
}

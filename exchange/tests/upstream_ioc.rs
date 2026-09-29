use nus_exchange_contract::adapter::Reservation;
use orderbook_rs::{
    OrderBook,
    orderbook::trade::TradeResult,
    prelude::{Id, OrderBookError, Side, TimeInForce},
};
use std::sync::{Arc, Mutex};
#[test]
fn real_ioc_error_retains_callback_debits() {
    let trades = Arc::new(Mutex::new(Vec::<TradeResult>::new()));
    let captured = trades.clone();
    let book = OrderBook::<()>::with_trade_listener(
        "DEVBASE/DEVQUOTE",
        Arc::new(move |tr| captured.lock().unwrap().push(tr.clone())),
    );
    let maker = Id::new_uuid();
    let taker = Id::new_uuid();
    book.add_limit_order(maker, 90, 3, Side::Sell, TimeInForce::Gtc, None)
        .unwrap();
    let result = book.add_limit_order(taker, 100, 5, Side::Buy, TimeInForce::Ioc, None);
    assert!(
        matches!(result, Err(OrderBookError::InsufficientLiquidity { .. })),
        "{result:?}"
    );
    let events = trades.lock().unwrap();
    assert!(!events.is_empty());
    let mut reservation = Reservation::buy(2, 5, 100).unwrap();
    let mut index = 0;
    for e in events.iter() {
        for trade in e.match_result.trades().as_vec() {
            reservation
                .trade(
                    2,
                    index,
                    trade.quantity().as_u64(),
                    u64::try_from(trade.price().as_u128()).unwrap(),
                )
                .unwrap();
            index += 1;
        }
    }
    assert_eq!(reservation.filled, 3);
    assert_eq!(reservation.complete_ioc(2), Ok(200));
    assert_eq!(
        (reservation.debit, reservation.pending_receive),
        (300, 3000)
    );
    assert!(book.get_order(taker).is_none());
    assert!(book.get_order(maker).is_none());
    assert_eq!(reservation.complete_ioc(2), Ok(0));
    println!(
        "PASS actual orderbook-rs 0.13.1: IOC requested=5, callback filled=3, Err, release R=200, retain D=300/P=3000"
    );
}
#[test]
fn real_cancel_and_zero_fill_ioc() {
    let book = OrderBook::<()>::new("DEVBASE/DEVQUOTE");
    let id = Id::new_uuid();
    book.add_limit_order(id, 90, 3, Side::Sell, TimeInForce::Gtc, None)
        .unwrap();
    assert!(book.cancel_order(id).unwrap().is_some());
    assert!(book.cancel_order(id).unwrap().is_none());
    let taker = Id::new_uuid();
    let result = book.add_limit_order(taker, 100, 5, Side::Buy, TimeInForce::Ioc, None);
    assert!(matches!(
        result,
        Err(OrderBookError::InsufficientLiquidity { .. })
    ));
    assert!(book.get_order(taker).is_none());
    let mut r = Reservation::buy(3, 5, 100).unwrap();
    assert_eq!(r.complete_ioc(3), Ok(500));
    assert_eq!((r.debit, r.pending_receive), (0, 0));
}

//! Deterministic candidate matching. The sequencer owns the canonical live book;
//! upstream is rebuilt privately for each command and never owns durable state.
//! A policy-approved prefix is fed to upstream so STP and fee rejection cannot
//! skip a maker. Upstream clocks/trade UUIDs never enter the canonical outcome.
use super::ledger::Side;
use crate::{Result, policy};
use orderbook_rs::{
    OrderBook,
    orderbook::trade::TradeResult,
    prelude::{Id, OrderBookError, Side as UpSide, TimeInForce},
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveOrder {
    pub hash: String,
    pub owner: String,
    pub admission_seq: u64,
    pub side: Side,
    pub price: u64,
    pub remaining: u64,
    pub expiry_height: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tif {
    Gtc,
    Ioc,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remainder {
    Resting,
    Filled,
    IocCancelled,
    SelfTradeCancelled,
    PolicyRejected,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub maker_hash: String,
    pub quantity: u64,
    pub price: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchOutcome {
    pub fills: Vec<Match>,
    pub expired_hashes: Vec<String>,
    pub remaining: u64,
    pub remainder: Remainder,
}
fn upstream_side(side: Side) -> UpSide {
    match side {
        Side::Buy => UpSide::Buy,
        Side::Sell => UpSide::Sell,
    }
}
fn validate_order(o: &LiveOrder) -> Result<()> {
    if o.admission_seq == 0
        || o.hash.is_empty()
        || o.owner.is_empty()
        || !(1..=1_000_000).contains(&o.price)
        || !(1..=1_000_000).contains(&o.remaining)
    {
        return Err("ADAPTER_INVALID_ORDER");
    }
    Ok(())
}
/// Input must be the sequencer's authenticated, reserved candidate order and
/// complete live book at the recorded height. This function has no side effects.
pub fn execute(
    live: &[LiveOrder],
    taker: &LiveOrder,
    tif: Tif,
    height: u64,
    bps: u32,
) -> Result<MatchOutcome> {
    validate_order(taker)?;
    if taker.expiry_height <= height {
        return Err("EXPIRED");
    }
    if bps > 10_000 {
        return Err("BPS_RANGE");
    }
    let mut hashes = BTreeSet::from([taker.hash.as_str()]);
    let mut seqs = BTreeSet::from([taker.admission_seq]);
    let mut expired = Vec::new();
    let mut makers = Vec::new();
    for o in live {
        validate_order(o)?;
        if o.admission_seq >= taker.admission_seq
            || !hashes.insert(&o.hash)
            || !seqs.insert(o.admission_seq)
        {
            return Err("ADAPTER_ID_CONFLICT");
        }
        if o.expiry_height <= height {
            expired.push(o);
        } else if o.side != taker.side
            && match taker.side {
                Side::Buy => o.price <= taker.price,
                Side::Sell => o.price >= taker.price,
            }
        {
            makers.push(o);
        }
    }
    expired.sort_by_key(|o| o.admission_seq);
    makers.sort_by(|a, b| {
        let price = match taker.side {
            Side::Buy => a.price.cmp(&b.price),
            Side::Sell => b.price.cmp(&a.price),
        };
        price.then(a.admission_seq.cmp(&b.admission_seq))
    });
    let mut remaining = taker.remaining;
    let mut remainder = match tif {
        Tif::Gtc => Remainder::Resting,
        Tif::Ioc => Remainder::IocCancelled,
    };
    let mut expected = Vec::new();
    let mut selected = Vec::new();
    for maker in makers {
        if remaining == 0 {
            break;
        }
        if maker.owner == taker.owner {
            remainder = Remainder::SelfTradeCancelled;
            break;
        }
        let q = remaining.min(maker.remaining);
        if policy::fill(q, maker.price, bps).is_err() {
            remainder = Remainder::PolicyRejected;
            break;
        }
        expected.push(Match {
            maker_hash: maker.hash.clone(),
            quantity: q,
            price: maker.price,
        });
        selected.push(maker);
        remaining -= q;
    }
    if remaining == 0 {
        remainder = Remainder::Filled;
    }
    // No durable/native UUID conversion: unique admission seq is the temporary
    // library ID. Selected makers are inserted in canonical FIFO order.
    selected.sort_by_key(|o| o.admission_seq);
    let events = Arc::new(Mutex::new(Vec::<TradeResult>::new()));
    let capture = events.clone();
    let book = OrderBook::<()>::with_trade_listener(
        "DEVBASE/DEVQUOTE",
        Arc::new(move |t| capture.lock().unwrap().push(t.clone())),
    );
    for maker in &selected {
        book.add_limit_order(
            Id::from_u64(maker.admission_seq),
            maker.price.into(),
            maker.remaining,
            upstream_side(maker.side),
            TimeInForce::Gtc,
            None,
        )
        .map_err(|_| "ADAPTER_RECOVERY_REQUIRED")?;
    }
    let returned = book.add_limit_order_with_result(
        Id::from_u64(taker.admission_seq),
        taker.price.into(),
        taker.remaining,
        upstream_side(taker.side),
        TimeInForce::Ioc,
        None,
    );
    let normalize = |events: &[TradeResult]| -> Result<Vec<Match>> {
        let mut fills = Vec::new();
        for event in events {
            if event.symbol != "DEVBASE/DEVQUOTE"
                || event.match_result.order_id() != Id::from_u64(taker.admission_seq)
            {
                return Err("ADAPTER_RECOVERY_REQUIRED");
            }
            for t in event.match_result.trades().as_vec() {
                let maker = selected
                    .iter()
                    .find(|o| Id::from_u64(o.admission_seq) == t.maker_order_id())
                    .ok_or("ADAPTER_RECOVERY_REQUIRED")?;
                if t.taker_order_id() != Id::from_u64(taker.admission_seq)
                    || t.taker_side() != upstream_side(taker.side)
                    || t.maker_side() != upstream_side(maker.side)
                {
                    return Err("ADAPTER_RECOVERY_REQUIRED");
                }
                fills.push(Match {
                    maker_hash: maker.hash.clone(),
                    quantity: t.quantity().as_u64(),
                    price: u64::try_from(t.price().as_u128())
                        .map_err(|_| "ADAPTER_RECOVERY_REQUIRED")?,
                });
            }
        }
        Ok(fills)
    };
    let callbacks = normalize(&events.lock().map_err(|_| "ADAPTER_RECOVERY_REQUIRED")?)?;
    match returned {
        Ok((_, result)) => {
            if remaining != 0 || normalize(&result.into_iter().collect::<Vec<_>>())? != callbacks {
                return Err("ADAPTER_RECOVERY_REQUIRED");
            }
        }
        Err(OrderBookError::InsufficientLiquidity { .. }) if remaining > 0 => {}
        Err(_) => return Err("ADAPTER_RECOVERY_REQUIRED"),
    }
    // Comparing the complete ordered output rejects duplicate callbacks as well
    // as missing/excess fills. Return and callback are never summed together.
    if callbacks != expected {
        return Err("ADAPTER_RECOVERY_REQUIRED");
    }
    Ok(MatchOutcome {
        fills: callbacks,
        expired_hashes: expired.into_iter().map(|o| o.hash.clone()).collect(),
        remaining,
        remainder,
    })
}

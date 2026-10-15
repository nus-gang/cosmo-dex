//! Private S3 accounting candidates, without RPC/proof authority. Callers must
//! verify receipt and same-height C, then journal the entire candidate before ACK.
use crate::{Result, policy};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Quantities {
    pub lifetime_matched: u64,
    pub pending: u64,
    pub settled: u64,
    pub corrected: u64,
    pub remaining: u64,
    pub cancelled: u64,
}

use super::dependencies::State;
pub use crate::s2::ledger::{Asset, Balance, Side};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Reservation {
    owner: String,
    side: Side,
    price: u64,
    remaining: u64,
    original: u64,
    matched: u64,
    cancelled: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fill {
    pub buyer: String,
    pub seller: String,
    pub buy_order: String,
    pub sell_order: String,
    pub quantity: u64,
    pub price: u64,
    pub buy_debit: u128,
    pub sell_debit: u128,
    pub base_net: u128,
    pub quote_net: u128,
    pub base_fee: u128,
    pub quote_fee: u128,
    pub state: State,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    balances: BTreeMap<(String, Asset), Balance>,
    reservations: BTreeMap<String, Reservation>,
    fills: BTreeMap<String, Fill>,
}
fn add(a: u128, b: u128) -> Result<u128> {
    a.checked_add(b).ok_or("INTEGER_OVERFLOW")
}
fn sub(a: u128, b: u128) -> Result<u128> {
    a.checked_sub(b).ok_or("LEDGER_UNDERFLOW")
}
fn input(side: Side, q: u64, p: u64) -> Result<u128> {
    policy::checked_product(q.into(), if side == Side::Buy { p.into() } else { 1000 }, 1)
}
fn asset(side: Side) -> Asset {
    if side == Side::Buy {
        Asset::Quote
    } else {
        Asset::Base
    }
}
impl Ledger {
    /// Recompute lifetime accounting from original fills; terminalization never
    /// restores quantity, so corrected orders cannot be matched a second time.
    pub fn quantities(&self, order: &str) -> Result<Quantities> {
        let r = self.reservations.get(order).ok_or("ORDER_NOT_FOUND")?;
        let mut q = Quantities {
            lifetime_matched: r.matched,
            remaining: r.remaining,
            cancelled: r.cancelled,
            ..Quantities::default()
        };
        for fill in self
            .fills
            .values()
            .filter(|f| f.buy_order == order || f.sell_order == order)
        {
            let target = match fill.state {
                State::Pending | State::SubmissionUnknown => &mut q.pending,
                State::Committed => &mut q.settled,
                State::Corrected => &mut q.corrected,
            };
            *target = target
                .checked_add(fill.quantity)
                .ok_or("INTEGER_OVERFLOW")?;
        }
        if q.pending
            .checked_add(q.settled)
            .and_then(|n| n.checked_add(q.corrected))
            != Some(r.matched)
            || r.matched
                .checked_add(r.remaining)
                .and_then(|n| n.checked_add(r.cancelled))
                != Some(r.original)
        {
            return Err("CUMULATIVE_RECONCILIATION");
        }
        Ok(q)
    }
    pub fn pending_fees(&self) -> Result<[u128; 2]> {
        let mut fees = [0; 2];
        for f in self.fills.values().filter(|f| f.state.pending()) {
            fees[0] = add(fees[0], f.base_fee)?;
            fees[1] = add(fees[1], f.quote_fee)?;
        }
        Ok(fees)
    }
    /// Arithmetic part of one engine snapshot commit. Inputs have no implicit
    /// proof authority: the sequencer must verify receipts/VOID/height/cursor.
    /// The complete owner set is required. If the newest C cannot support the
    /// surviving R+D, nothing is changed; the caller retains a frozen old view.
    pub fn reconcile(
        &mut self,
        confirmed: &BTreeMap<String, [u128; 2]>,
        committed: &[String],
        corrected: &[String],
        cancel_orders: &[String],
    ) -> Result<()> {
        self.transaction(|next| {
            if confirmed.len().checked_mul(2) != Some(next.balances.len()) {
                return Err("SNAPSHOT_OWNER_SET");
            }
            let mut seen = std::collections::BTreeSet::new();
            for (ids, state) in [(committed, State::Committed), (corrected, State::Corrected)] {
                for id in ids {
                    if !seen.insert(id) {
                        return Err("DUPLICATE_FILL");
                    }
                    let f = next.fills.get_mut(id).ok_or("FILL_NOT_FOUND")?;
                    if f.state != state && !f.state.pending() {
                        return Err("TERMINAL_FILL_IMMUTABLE");
                    }
                    f.state = state;
                }
            }
            let mut cancelled = std::collections::BTreeSet::new();
            for id in cancel_orders {
                if !cancelled.insert(id) {
                    return Err("DUPLICATE_ORDER");
                }
                let r = next.reservations.get_mut(id).ok_or("ORDER_NOT_FOUND")?;
                r.cancelled = r
                    .cancelled
                    .checked_add(r.remaining)
                    .ok_or("INTEGER_OVERFLOW")?;
                r.remaining = 0;
            }
            // Rebuild, instead of adding provisional P to C or incrementally
            // subtracting D under a possibly inconsistent intermediate C.
            let rebuilt = Self::new(
                confirmed
                    .iter()
                    .map(|(owner, c)| (owner.clone(), c[0], c[1])),
            )?;
            if next.balances.keys().ne(rebuilt.balances.keys()) {
                return Err("SNAPSHOT_OWNER_SET");
            }
            next.balances = rebuilt.balances;
            for r in next.reservations.values() {
                let b = next
                    .balances
                    .get_mut(&(r.owner.clone(), asset(r.side)))
                    .ok_or("UNKNOWN_OWNER")?;
                b.r = add(b.r, input(r.side, r.remaining, r.price)?)?;
            }
            for f in next.fills.values().filter(|f| f.state.pending()) {
                for (owner, asset, d, p) in [
                    (&f.buyer, Asset::Quote, f.buy_debit, 0),
                    (&f.seller, Asset::Base, f.sell_debit, 0),
                    (&f.buyer, Asset::Base, 0, f.base_net),
                    (&f.seller, Asset::Quote, 0, f.quote_net),
                ] {
                    let b = next
                        .balances
                        .get_mut(&(owner.clone(), asset))
                        .ok_or("UNKNOWN_OWNER")?;
                    b.d = add(b.d, d)?;
                    b.p = add(b.p, p)?;
                }
            }
            Ok(())
        })
    }
    /// Bootstrap only. Subsequent snapshots require sequencer epoch correction.
    pub fn new(accounts: impl IntoIterator<Item = (String, u128, u128)>) -> Result<Self> {
        let mut state = Self::default();
        for (owner, base, quote) in accounts {
            if state.balances.contains_key(&(owner.clone(), Asset::Base)) {
                return Err("DUPLICATE_OWNER");
            }
            for (asset, c) in [(Asset::Base, base), (Asset::Quote, quote)] {
                state.balances.insert(
                    (owner.clone(), asset),
                    Balance {
                        c,
                        ..Balance::default()
                    },
                );
            }
        }
        Ok(state)
    }
    pub fn balance(&self, owner: &str, asset: Asset) -> Result<&Balance> {
        self.balances
            .get(&(owner.to_owned(), asset))
            .ok_or("UNKNOWN_OWNER")
    }
    fn balance_mut(&mut self, owner: &str, asset: Asset) -> Result<&mut Balance> {
        self.balances
            .get_mut(&(owner.to_owned(), asset))
            .ok_or("UNKNOWN_OWNER")
    }
    pub fn fills(&self) -> &BTreeMap<String, Fill> {
        &self.fills
    }
    pub fn fill(&self, id: &str) -> Option<&Fill> {
        self.fills.get(id)
    }
    pub fn remaining(&self, order: &str) -> Option<u64> {
        self.reservations.get(order).map(|r| r.remaining)
    }
    fn transaction<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let mut next = self.clone();
        let result = f(&mut next)?;
        next.validate()?;
        *self = next;
        Ok(result)
    }
    /// Reconcile every balance against all live reservation/fill contributions.
    pub fn validate(&self) -> Result<()> {
        let mut expected = Self::new(
            self.balances
                .iter()
                .filter(|((_, asset), _)| *asset == Asset::Base)
                .map(|((owner, _), base)| {
                    (
                        owner.clone(),
                        base.c,
                        self.balances[&(owner.clone(), Asset::Quote)].c,
                    )
                }),
        )?;
        for r in self.reservations.values() {
            let b = expected.balance_mut(&r.owner, asset(r.side))?;
            b.r = add(b.r, input(r.side, r.remaining, r.price)?)?;
        }
        for f in self.fills.values().filter(|f| f.state.pending()) {
            for (owner, asset, debit, pending) in [
                (&f.buyer, Asset::Quote, f.buy_debit, 0),
                (&f.seller, Asset::Base, f.sell_debit, 0),
                (&f.buyer, Asset::Base, 0, f.base_net),
                (&f.seller, Asset::Quote, 0, f.quote_net),
            ] {
                let b = expected.balance_mut(owner, asset)?;
                b.d = add(b.d, debit)?;
                b.p = add(b.p, pending)?;
            }
        }
        if expected.balances != self.balances {
            return Err("LEDGER_RECONCILIATION");
        }
        let mut matched = BTreeMap::<&str, u64>::new();
        for fill in self.fills.values() {
            for order in [&fill.buy_order, &fill.sell_order] {
                if !self.reservations.contains_key(order) {
                    return Err("ORDER_NOT_FOUND");
                }
                let count = matched.entry(order).or_default();
                *count = count.checked_add(fill.quantity).ok_or("INTEGER_OVERFLOW")?;
            }
        }
        for (order, r) in &self.reservations {
            if matched.get(order.as_str()).copied().unwrap_or(0) != r.matched
                || r.matched
                    .checked_add(r.remaining)
                    .and_then(|n| n.checked_add(r.cancelled))
                    != Some(r.original)
            {
                return Err("CUMULATIVE_RECONCILIATION");
            }
        }
        for b in self.balances.values() {
            b.available()?;
        }
        Ok(())
    }
    pub fn reserve(
        &mut self,
        order: &str,
        owner: &str,
        side: Side,
        q: u64,
        price: u64,
    ) -> Result<()> {
        self.transaction(|next| {
            if next.reservations.contains_key(order) {
                return Err("ORDER_ALREADY_BOUND");
            }
            if !(1..=1_000_000).contains(&q) || !(1..=1_000_000).contains(&price) {
                return Err("MARKET_LIMIT");
            }
            let amount = input(side, q, price)?;
            let b = next.balance_mut(owner, asset(side))?;
            if amount > b.available()? {
                return Err("INSUFFICIENT_AVAILABLE");
            }
            b.r = add(b.r, amount)?;
            next.reservations.insert(
                order.into(),
                Reservation {
                    owner: owner.into(),
                    side,
                    price,
                    remaining: q,
                    original: q,
                    matched: 0,
                    cancelled: 0,
                },
            );
            Ok(())
        })
    }
    /// Cancel/IOC/expiry release only unfilled R. Tombstones prevent ID reuse.
    pub fn cancel(&mut self, order: &str) -> Result<()> {
        self.transaction(|next| {
            let r = next
                .reservations
                .get(order)
                .ok_or("ORDER_NOT_FOUND")?
                .clone();
            let b = next.balance_mut(&r.owner, asset(r.side))?;
            b.r = sub(b.r, input(r.side, r.remaining, r.price)?)?;
            let reservation = next.reservations.get_mut(order).unwrap();
            reservation.cancelled = reservation
                .cancelled
                .checked_add(reservation.remaining)
                .ok_or("INTEGER_OVERFLOW")?;
            reservation.remaining = 0;
            Ok(())
        })
    }
    /// Adapter deduplication happens before accounting; repeated fill IDs fail
    /// closed here so a repeated callback can never create a second debit.
    pub fn match_fill(
        &mut self,
        id: &str,
        buy: &str,
        sell: &str,
        q: u64,
        price: u64,
        bps: u32,
    ) -> Result<()> {
        self.transaction(|next| {
            if next.fills.contains_key(id) {
                return Err("FILL_ALREADY_BOUND");
            }
            let b = next.reservations.get(buy).ok_or("ORDER_NOT_FOUND")?.clone();
            let s = next
                .reservations
                .get(sell)
                .ok_or("ORDER_NOT_FOUND")?
                .clone();
            if b.side != Side::Buy || s.side != Side::Sell {
                return Err("SIDE_MISMATCH");
            }
            if b.owner == s.owner {
                return Err("SELF_TRADE");
            }
            if price > b.price || price < s.price {
                return Err("PRICE_LIMIT");
            }
            if q == 0 || q > b.remaining || q > s.remaining {
                return Err("CUMULATIVE_QTY_EXCEEDED");
            }
            let (base, quote, base_fee, quote_fee) = policy::fill(q, price, bps)?;
            let buy_debit = input(Side::Buy, q, b.price)?;
            for (owner, asset, debit) in [
                (&b.owner, Asset::Quote, buy_debit),
                (&s.owner, Asset::Base, base),
            ] {
                let balance = next.balance_mut(owner, asset)?;
                balance.r = sub(balance.r, debit)?;
                balance.d = add(balance.d, debit)?;
            }
            let buyer = next.balance_mut(&b.owner, Asset::Base)?;
            buyer.p = add(buyer.p, base - base_fee)?;
            let seller = next.balance_mut(&s.owner, Asset::Quote)?;
            seller.p = add(seller.p, quote - quote_fee)?;
            for order in [buy, sell] {
                let r = next.reservations.get_mut(order).unwrap();
                r.remaining -= q;
                r.matched = r.matched.checked_add(q).ok_or("INTEGER_OVERFLOW")?;
            }
            next.fills.insert(
                id.into(),
                Fill {
                    buyer: b.owner,
                    seller: s.owner,
                    buy_order: buy.into(),
                    sell_order: sell.into(),
                    quantity: q,
                    price,
                    buy_debit,
                    sell_debit: base,
                    base_net: base - base_fee,
                    quote_net: quote - quote_fee,
                    base_fee,
                    quote_fee,
                    state: State::Pending,
                },
            );
            Ok(())
        })
    }
}

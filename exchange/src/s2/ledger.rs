//! Candidate-state accounting for the S2 sequencer. This module never publishes
//! receipts or changes chain balances. The caller must journal before publication.
use crate::{Result, policy};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Asset {
    Base,
    Quote,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Balance {
    pub c: u128,
    pub r: u128,
    pub d: u128,
    pub p: u128,
}
impl Balance {
    pub fn available(&self) -> Result<u128> {
        self.c
            .checked_sub(self.r)
            .and_then(|n| n.checked_sub(self.d))
            .ok_or("INSUFFICIENT_AVAILABLE")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Reservation {
    owner: String,
    side: Side,
    price: u64,
    remaining: u64,
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
    pub corrected: bool,
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
    /// Only after the sequencer has corrected epoch-dependent obligations.
    pub(crate) fn set_confirmed(&mut self, owner: &str, amounts: [u128; 2]) -> Result<()> {
        self.transaction(|next| {
            for (asset, c) in [(Asset::Base, amounts[0]), (Asset::Quote, amounts[1])] {
                next.balance_mut(owner, asset)?.c = c;
            }
            Ok(())
        })
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
        for f in self.fills.values().filter(|f| !f.corrected) {
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
            next.reservations.get_mut(order).unwrap().remaining = 0;
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
            if q > b.remaining || q > s.remaining {
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
            next.reservations.get_mut(buy).unwrap().remaining -= q;
            next.reservations.get_mut(sell).unwrap().remaining -= q;
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
                    corrected: false,
                },
            );
            Ok(())
        })
    }
    /// Exact reversal of stored contributions; never restores order quantities.
    /// The sequencer owns the full connected-component correction transaction.
    pub fn correct_fill(&mut self, id: &str) -> Result<()> {
        self.transaction(|next| {
            let fill = next.fills.get(id).ok_or("FILL_NOT_FOUND")?.clone();
            if fill.corrected {
                return Ok(());
            }
            for (owner, asset, debit) in [
                (&fill.buyer, Asset::Quote, fill.buy_debit),
                (&fill.seller, Asset::Base, fill.sell_debit),
            ] {
                let b = next.balance_mut(owner, asset)?;
                b.d = sub(b.d, debit)?;
            }
            for (owner, asset, net) in [
                (&fill.buyer, Asset::Base, fill.base_net),
                (&fill.seller, Asset::Quote, fill.quote_net),
            ] {
                let b = next.balance_mut(owner, asset)?;
                b.p = sub(b.p, net)?;
            }
            next.fills.get_mut(id).unwrap().corrected = true;
            Ok(())
        })
    }
}

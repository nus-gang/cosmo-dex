//! Sequencer-owned in-memory adapter contract. Persistence is deliberately external.
use crate::{Result, policy::cumulative};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub remaining: u128,
    pub debit: u128,
    pub pending_receive: u128,
    pub filled: u64,
    pub max_qty: u64,
    pub limit_price: u64,
    pub command_seq: u64,
    closed: bool,
    seen: BTreeMap<u32, (u64, u64)>,
}
impl Reservation {
    pub fn buy(command_seq: u64, qty: u64, limit: u64) -> Result<Self> {
        if qty == 0 || limit == 0 {
            return Err("MARKET_LIMIT");
        }
        Ok(Self {
            remaining: qty as u128 * limit as u128,
            debit: 0,
            pending_receive: 0,
            filled: 0,
            max_qty: qty,
            limit_price: limit,
            command_seq,
            closed: false,
            seen: BTreeMap::new(),
        })
    }
    /// A trade callback consumes worst-case debit, not improved execution price.
    pub fn trade(&mut self, seq: u64, index: u32, qty: u64, price: u64) -> Result<()> {
        if seq != self.command_seq {
            return Err("COMMAND_SEQUENCE_MISMATCH");
        }
        if let Some(old) = self.seen.get(&index) {
            return if *old == (qty, price) {
                Ok(())
            } else {
                Err("ID_CONFLICT")
            };
        }
        if self.closed {
            return Err("CALLBACK_AFTER_COMPLETE");
        }
        if index as usize != self.seen.len() {
            return Err("MATCH_SEQUENCE_GAP");
        }
        if qty == 0 || price == 0 || price > self.limit_price {
            return Err("MARKET_LIMIT");
        }
        let filled = cumulative(self.filled, qty, self.max_qty)?;
        let d = qty as u128 * self.limit_price as u128;
        let r = self
            .remaining
            .checked_sub(d)
            .ok_or("INSUFFICIENT_CONFIRMED_BALANCE")?;
        let debit = self.debit.checked_add(d).ok_or("INTEGER_RANGE")?;
        let pending = self
            .pending_receive
            .checked_add(qty as u128 * 1000)
            .ok_or("INTEGER_RANGE")?;
        self.remaining = r;
        self.debit = debit;
        self.pending_receive = pending;
        self.filled = filled;
        self.seen.insert(index, (qty, price));
        Ok(())
    }
    /// Only after all callbacks of this sequenced command, even if upstream returned Err.
    pub fn complete_ioc(&mut self, seq: u64) -> Result<u128> {
        if seq != self.command_seq {
            return Err("COMMAND_SEQUENCE_MISMATCH");
        }
        if self.closed {
            return Ok(0);
        }
        let release = self.remaining;
        self.remaining = 0;
        self.closed = true;
        Ok(release)
    }
}

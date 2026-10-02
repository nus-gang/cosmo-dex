//! Private candidate transitions, not a service or durable receipt boundary.
//! The owner must journal input/result/outbox before publishing a returned state.
use super::{
    journal::sha256,
    ledger::{Asset, Ledger, Side},
    matching::{self, LiveOrder, Remainder, Tif},
    snapshot::{Advance, Observation, Snapshot},
};
use crate::{
    Result,
    codec::{self, Codec, integer},
    policy,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub seq: u64,
    pub hash: String,
    pub code: String,
    pub fills: Vec<String>,
}
#[derive(Clone, Debug)]
struct Bound {
    hash: String,
    outcome: Outcome,
    // Keep original authenticated evidence; never replace it on retries.
    raw: Vec<u8>,
    signature: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Order {
    pub live: LiveOrder,
    pub epoch: u64,
    pub order_id: String,
    pub status: String,
    pub filled: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Correction {
    pub affected_owners: Vec<String>,
    pub affected_order_hashes: Vec<String>,
    pub cancelled_order_hashes: Vec<String>,
    pub corrected_fill_ids: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    snapshot: Snapshot,
    ledger: Ledger,
    seq: u64,
    bindings: BTreeMap<(String, String, String), Bound>,
    orders: BTreeMap<String, Order>,
    frozen: BTreeMap<String, u64>,
    fill_order: Vec<String>,
}
impl Candidate {
    pub fn new(snapshot: Snapshot) -> Result<Self> {
        let ledger = Ledger::new(
            snapshot
                .accounts()
                .iter()
                .map(|a| (a.owner.clone(), a.confirmed[0], a.confirmed[1])),
        )?;
        Ok(Self {
            snapshot,
            ledger,
            seq: 0,
            bindings: BTreeMap::new(),
            orders: BTreeMap::new(),
            frozen: BTreeMap::new(),
            fill_order: Vec::new(),
        })
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn is_frozen(&self, owner: &str) -> bool {
        self.frozen.contains_key(owner)
    }
    /// Private atomic correction candidate. The caller must persist the input,
    /// correction, state and outbox together before publishing this snapshot.
    pub fn advance(&self, snapshot: Snapshot) -> Result<(Self, Correction)> {
        let mut correction = Correction {
            affected_owners: vec![],
            affected_order_hashes: vec![],
            cancelled_order_hashes: vec![],
            corrected_fill_ids: vec![],
        };
        let changed = match self.snapshot.advance(&snapshot)? {
            Advance::Duplicate => return Ok((self.clone(), correction)),
            Advance::Next {
                epoch_changed_owners,
            } => epoch_changed_owners,
        };
        let mut owners: BTreeSet<String> = changed.into_iter().collect();
        loop {
            let before = owners.len();
            for fill in self.ledger.fills().values().filter(|f| !f.corrected) {
                if owners.contains(&fill.buyer) || owners.contains(&fill.seller) {
                    owners.insert(fill.buyer.clone());
                    owners.insert(fill.seller.clone());
                }
            }
            if owners.len() == before {
                break;
            }
        }
        let mut next = self.clone();
        next.seq = next.seq.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
        let mut affected = BTreeSet::new();
        for id in &self.fill_order {
            let fill = self.ledger.fill(id).ok_or("LEDGER_RECONCILIATION")?;
            if !fill.corrected && owners.contains(&fill.buyer) {
                affected.insert(fill.buy_order.clone());
                affected.insert(fill.sell_order.clone());
                next.ledger.correct_fill(id)?;
                correction.corrected_fill_ids.push(id.clone());
            }
        }
        let mut ordered: Vec<_> = self.orders.values().collect();
        ordered.sort_by_key(|o| o.live.admission_seq);
        for old in ordered {
            let hash = &old.live.hash;
            let impacted = owners.contains(&old.live.owner);
            let expired = old.live.expiry_height <= snapshot.height();
            if old.live.remaining > 0 && (impacted || expired) {
                next.ledger.cancel(hash)?;
                let order = next.orders.get_mut(hash).unwrap();
                order.live.remaining = 0;
                order.status = if impacted { "CORRECTED" } else { "EXPIRED" }.into();
                if impacted {
                    affected.insert(hash.clone());
                    correction.cancelled_order_hashes.push(hash.clone());
                }
            }
            if affected.contains(hash) {
                next.orders.get_mut(hash).unwrap().status = "CORRECTED".into();
                correction.affected_order_hashes.push(hash.clone());
            }
        }
        for a in snapshot.accounts() {
            next.ledger.set_confirmed(&a.owner, a.confirmed)?;
        }
        next.ledger.validate()?;
        next.snapshot = snapshot;
        correction.affected_owners = owners.into_iter().collect();
        Ok((next, correction))
    }
    /// Authenticated API session owner; no authority to sign or gate chain TXs.
    pub fn prepare_withdraw(&self, owner: &str) -> Result<(Self, &'static str)> {
        self.ledger.balance(owner, Asset::Base)?;
        let mut next = self.clone();
        next.seq = next.seq.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
        next.frozen.insert(owner.into(), self.snapshot.height());
        for order in next
            .orders
            .values_mut()
            .filter(|o| o.live.owner == owner && o.live.remaining > 0)
        {
            next.ledger.cancel(&order.live.hash)?;
            order.live.remaining = 0;
            order.status = "CANCELLED_OFFCHAIN".into();
        }
        let hold = [Asset::Base, Asset::Quote].into_iter().any(|asset| {
            let b = next.ledger.balance(owner, asset).unwrap();
            b.d != 0 || b.p != 0
        });
        next.ledger.validate()?;
        Ok((next, if hold { "UNSETTLED_HOLD" } else { "OK" }))
    }
    pub fn abort_withdraw(&self, owner: &str, observation: &Observation, now: u64) -> Result<Self> {
        let height = self.frozen.get(owner).ok_or("WITHDRAW_NOT_PREPARED")?;
        if self.snapshot.height() <= *height {
            return Err("STALE");
        }
        self.snapshot.freshness(observation, now)?;
        self.ledger.validate()?;
        let mut next = self.clone();
        next.seq = next.seq.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
        next.frozen.remove(owner);
        Ok(next)
    }
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }
    pub fn sequence(&self) -> u64 {
        self.seq
    }
    pub fn orders(&self) -> &BTreeMap<String, Order> {
        &self.orders
    }
    pub fn evidence(&self, kind: &str, owner: &str, id: &str) -> Option<(&[u8], &[u8])> {
        self.bindings
            .get(&(kind.into(), owner.into(), id.into()))
            .map(|b| (b.raw.as_slice(), b.signature.as_slice()))
    }
    /// `session_owner` is authenticated by the API, never copied from the body.
    /// Authentication/conflict errors have no binding. Deterministic policy
    /// rejection becomes a candidate command; persistence is still required.
    pub fn submit(
        &self,
        kind: &str,
        raw: &[u8],
        sig: &[u8],
        session_owner: &str,
        observation: &Observation,
        now: u64,
    ) -> Result<(Self, Outcome, bool)> {
        let (name, domain) = match kind {
            "ORDER" => ("OrderV1", "NUS/ORDER/V1"),
            "CANCEL" => ("CancelV1", "NUS/CANCEL/V1"),
            _ => return Err("UNSUPPORTED_VERSION"),
        };
        let v = Codec::default().decode(name, raw)?;
        if v["protocol_version"] != "1" {
            return Err("UNSUPPORTED_VERSION");
        }
        let context = &self.snapshot.value()["body"]["context"];
        for key in ["chain_id", "genesis_hash", "market_id"] {
            if v[key] != context[key] {
                return Err("CONTEXT_MISMATCH");
            }
        }
        if v["exchange_module_id"] != "x/exchange"
            || (kind == "ORDER" && v["market_config_version"] != context["market_config_version"])
        {
            return Err("CONTEXT_MISMATCH");
        }
        let owner = v["owner"].as_str().ok_or("ADDRESS_MISMATCH")?;
        if owner != session_owner {
            return Err("FORBIDDEN");
        }
        let account = self
            .snapshot
            .accounts()
            .iter()
            .find(|a| a.owner == owner)
            .ok_or("ACCOUNT_KEY_UNREGISTERED")?;
        let bps = integer(&self.snapshot.value()["body"]["market"]["fee_bps"], 32)? as u32;
        if kind == "ORDER" {
            let ctx = policy::OrderContext {
                snapshot_id: self.snapshot.id(),
                chain_id: context["chain_id"].as_str().unwrap(),
                genesis_hash: context["genesis_hash"].as_str().unwrap(),
                exchange_module_id: "x/exchange",
                market_id: "DEVBASE/DEVQUOTE",
                market_config_version: 1,
                registered_key_type: Some("ML-DSA-65"),
                registered_key: Some(&account.public_key),
                epoch: account.epoch,
                height: self.snapshot.height(),
                revoked: false,
                filled: 0,
                available: 0,
                fee_bps: bps,
            };
            policy::authenticate_order(raw, sig, &ctx)?;
        } else {
            if sig.len() != 3309 {
                return Err("KEY_LENGTH");
            }
            if !codec::verify_raw(&account.public_key, &codec::frame(domain, raw), sig, &[]) {
                return Err("INVALID_SIGNATURE");
            }
        }
        let hash = sha256(&codec::frame(domain, raw));
        let id = if kind == "ORDER" {
            format!(
                "{}:{}",
                v["owner_epoch"].as_str().unwrap(),
                v["order_id"].as_str().unwrap()
            )
        } else {
            v["cancel_nonce"].as_str().unwrap().into()
        };
        let key = (kind.into(), owner.into(), id);
        if let Some(bound) = self.bindings.get(&key) {
            if bound.hash != hash {
                return Err("ID_CONFLICT");
            }
            return Ok((self.clone(), bound.outcome.clone(), true));
        }
        let mut next = self.clone();
        next.seq = self.seq.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
        let mut outcome = Outcome {
            seq: next.seq,
            hash: hash.clone(),
            code: "OK".into(),
            fills: vec![],
        };
        let effect = (|| {
            if integer(&v["owner_epoch"], 64)? != account.epoch as u128 {
                return Err("EPOCH_MISMATCH");
            }
            policy::expiry(
                self.snapshot.height(),
                integer(&v["expiry_height"], 64)? as u64,
            )?;
            if kind == "ORDER" {
                let delta = (integer(&v["expiry_height"], 64)? as u64) - self.snapshot.height();
                if !(2..=1000).contains(&delta) {
                    return Err("EXPIRY_MARGIN");
                }
                policy::market_rules(&v)?;
                if u128::from(bps) > integer(&v["max_fee_bps"], 32)? {
                    return Err("FEE_CAP");
                }
                let q = integer(&v["max_qty_lots"], 64)? as u64;
                let p = integer(&v["limit_price_ticks"], 64)? as u64;
                policy::fill(q, p, bps)?;
                self.snapshot.freshness(observation, now)?;
                if self.is_frozen(owner) {
                    return Err("WITHDRAW_FROZEN");
                }
                next.place(&v, &hash, bps, &mut outcome)?;
            } else {
                next.cancel(&v)?;
            }
            next.ledger.validate()
        })();
        if let Err(code) = effect {
            // Adapter/invariant failures must not become ordinary rejections.
            if ![
                "WITHDRAW_FROZEN",
                "EPOCH_MISMATCH",
                "EXPIRED",
                "EXPIRY_MARGIN",
                "MARKET_LIMIT",
                "FEE_CAP",
                "FEE_GE_RECEIVE",
                "STALE",
                "CATCHING_UP",
                "SNAPSHOT_CONFLICT",
                "INSUFFICIENT_AVAILABLE",
                "OPEN_ORDER_LIMIT",
                "ORDER_NOT_FOUND",
                "ID_CONFLICT",
            ]
            .contains(&code)
            {
                return Err(code);
            }
            next = self.clone();
            next.seq = outcome.seq;
            outcome.code = code.into();
            outcome.fills.clear();
        }
        next.bindings.insert(
            key,
            Bound {
                hash,
                outcome: outcome.clone(),
                raw: raw.to_vec(),
                signature: sig.to_vec(),
            },
        );
        Ok((next, outcome, false))
    }
    fn place(&mut self, v: &Value, hash: &str, bps: u32, outcome: &mut Outcome) -> Result<()> {
        let owner = v["owner"].as_str().unwrap();
        // Expiry and the admitted command are one candidate transition.
        for o in self
            .orders
            .values_mut()
            .filter(|o| o.live.remaining > 0 && o.live.expiry_height <= self.snapshot.height())
        {
            self.ledger.cancel(&o.live.hash)?;
            o.live.remaining = 0;
            o.status = "EXPIRED".into();
        }
        let live: Vec<_> = self
            .orders
            .values()
            .filter(|o| o.live.remaining > 0)
            .map(|o| o.live.clone())
            .collect();
        if live.len() >= 200 || live.iter().filter(|o| o.owner == owner).count() >= 100 {
            return Err("OPEN_ORDER_LIMIT");
        }
        let side = if v["side"] == "1" {
            Side::Buy
        } else {
            Side::Sell
        };
        let taker = LiveOrder {
            hash: hash.into(),
            owner: owner.into(),
            admission_seq: self.seq,
            side,
            price: integer(&v["limit_price_ticks"], 64)? as u64,
            remaining: integer(&v["max_qty_lots"], 64)? as u64,
            expiry_height: integer(&v["expiry_height"], 64)? as u64,
        };
        let asset = if side == Side::Buy {
            Asset::Quote
        } else {
            Asset::Base
        };
        self.ledger.balance(owner, asset)?.available()?;
        self.ledger
            .reserve(hash, owner, side, taker.remaining, taker.price)?;
        let matched = matching::execute(
            &live,
            &taker,
            if v["order_type"] == "1" {
                Tif::Gtc
            } else {
                Tif::Ioc
            },
            self.snapshot.height(),
            bps,
        )?;
        for (index, f) in matched.fills.iter().enumerate() {
            let identity = json!({"chain_id":self.snapshot.value()["body"]["context"]["chain_id"], "market_id":"DEVBASE/DEVQUOTE", "operator_epoch":"1", "command_seq":self.seq.to_string(), "match_index":index.to_string()});
            let fill_id = sha256(&codec::frame(
                "NUS/FILL_ID/V1",
                &Codec::default().encode("FillIdentityV1", &identity)?,
            ));
            let (buy, sell) = if side == Side::Buy {
                (hash, f.maker_hash.as_str())
            } else {
                (f.maker_hash.as_str(), hash)
            };
            self.ledger
                .match_fill(&fill_id, buy, sell, f.quantity, f.price, bps)?;
            let maker = self
                .orders
                .get_mut(&f.maker_hash)
                .ok_or("ADAPTER_RECOVERY_REQUIRED")?;
            maker.live.remaining -= f.quantity;
            maker.filled = maker
                .filled
                .checked_add(f.quantity)
                .ok_or("INTEGER_OVERFLOW")?;
            maker.status = if maker.live.remaining == 0 {
                "FILLED_PENDING"
            } else {
                "PARTIALLY_FILLED"
            }
            .into();
            self.fill_order.push(fill_id.clone());
            outcome.fills.push(fill_id);
        }
        let filled = taker.remaining - matched.remaining;
        let mut order = Order {
            live: taker,
            epoch: integer(&v["owner_epoch"], 64)? as u64,
            order_id: v["order_id"].as_str().unwrap().into(),
            status: match matched.remainder {
                Remainder::Resting if filled > 0 => "PARTIALLY_FILLED",
                Remainder::Resting => "OPEN",
                Remainder::Filled => "FILLED_PENDING",
                Remainder::IocCancelled => "CANCELLED_OFFCHAIN",
                Remainder::SelfTradeCancelled => "STP_CANCELLED",
                Remainder::PolicyRejected => "POLICY_REJECTED_REMAINDER",
            }
            .into(),
            filled,
        };
        order.live.remaining = matched.remaining;
        if matched.remainder != Remainder::Resting {
            self.ledger.cancel(hash)?;
            order.live.remaining = 0;
        }
        self.orders.insert(hash.into(), order);
        Ok(())
    }
    fn cancel(&mut self, v: &Value) -> Result<()> {
        let owner = v["owner"].as_str().unwrap();
        let epoch = integer(&v["owner_epoch"], 64)? as u64;
        let order = self
            .orders
            .values_mut()
            .find(|o| {
                o.live.owner == owner
                    && o.epoch == epoch
                    && o.order_id == v["order_id"].as_str().unwrap()
            })
            .ok_or("ORDER_NOT_FOUND")?;
        if v["order_hash"] != order.live.hash {
            return Err("ID_CONFLICT");
        }
        if order.live.remaining > 0 {
            self.ledger.cancel(&order.live.hash)?;
            order.live.remaining = 0;
            order.status = "CANCELLED_OFFCHAIN".into();
        }
        Ok(())
    }
}

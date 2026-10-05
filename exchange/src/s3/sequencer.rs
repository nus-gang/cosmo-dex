//! Private candidate transitions, not a service or durable receipt boundary.
//! The owner must journal input/result/outbox before publishing a returned state.
use super::{
    dependencies::{Asset as DebitAsset, DebitDomain, Graph, Identity, State},
    journal::{canonical, sha256},
    ledger::{Asset, Ledger, Side},
    schema,
    snapshot::{Observation, Snapshot},
};
use crate::s2::matching::{self, LiveOrder, Remainder, Tif};
use crate::{
    Result,
    codec::{self, Codec, integer},
    policy,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
struct LocalBound {
    epoch: u64,
    outcome: Outcome,
    raw: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Order {
    pub live: LiveOrder,
    pub epoch: u64,
    pub order_id: String,
    pub status: String,
    pub filled: u64,
    pub revision: u64,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub(super) snapshot: Snapshot,
    pub(super) ledger: Ledger,
    pub(super) seq: u64,
    bindings: BTreeMap<(String, String, String), Bound>,
    local_bindings: BTreeMap<(String, String, String), LocalBound>,
    pub(super) orders: BTreeMap<String, Order>,
    frozen: BTreeMap<String, u64>,
    pub(super) graph: Graph,
    pub(super) batches: Vec<Value>,
    pub(super) batch_wires: BTreeMap<String, Vec<u8>>,
    pub(super) attempts: Vec<Value>,
    pub(super) resolutions: Vec<Value>,
    pub(super) applied: Vec<Value>,
    pub(super) corrections: Vec<Value>,
    pub(super) observations: Vec<Snapshot>,
    pub(super) history: Vec<Snapshot>,
    pub(super) failure_evidence: BTreeMap<String, Value>,
    pub(super) fill_order: Vec<String>,
    pub(super) outbox: BTreeMap<String, Value>,
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
            history: vec![snapshot.clone()],
            snapshot,
            ledger,
            seq: 0,
            bindings: BTreeMap::new(),
            local_bindings: BTreeMap::new(),
            orders: BTreeMap::new(),
            frozen: BTreeMap::new(),
            graph: Graph::default(),
            batches: vec![],
            batch_wires: BTreeMap::new(),
            attempts: vec![],
            resolutions: vec![],
            applied: vec![],
            corrections: vec![],
            observations: vec![],
            failure_evidence: BTreeMap::new(),
            fill_order: Vec::new(),
            outbox: BTreeMap::new(),
        })
    }
    fn corrected_quantities(&self) -> Result<BTreeMap<String, u64>> {
        let mut counts = BTreeMap::<String, u64>::new();
        for fill in self
            .ledger
            .fills()
            .values()
            .filter(|f| f.state == State::Corrected)
        {
            for hash in [&fill.buy_order, &fill.sell_order] {
                let count = counts.entry(hash.clone()).or_default();
                *count = count.checked_add(fill.quantity).ok_or("INTEGER_OVERFLOW")?;
            }
        }
        Ok(counts)
    }
    // Entity revision changes once per atomic command, even with multiple fills.
    pub(super) fn revise_orders(&mut self, before: &Self) -> Result<()> {
        let current = self.corrected_quantities()?;
        let previous = before.corrected_quantities()?;
        for (hash, old) in &before.orders {
            let corrected = current.get(hash) != previous.get(hash);
            let new = self.orders.get_mut(hash).ok_or("LEDGER_RECONCILIATION")?;
            if new.live.remaining != old.live.remaining
                || new.filled != old.filled
                || new.status != old.status
                || corrected
                || self.ledger.quantities(hash)? != before.ledger.quantities(hash)?
            {
                new.revision = old.revision.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
            }
        }
        Ok(())
    }
    /// Full internal state projection. Never expose this object to public clients:
    /// it contains both owners and signed order evidence. It is not a receipt or
    /// a standalone restore format: replay must recover result/local-action indexes.
    /// Mode is supplied by the service's admission gate, not inferred from balances.
    pub fn state_json(&self, mode: &str) -> Result<Value> {
        self.ledger.validate()?;
        let mut accounts = Vec::new();
        for a in self.snapshot.accounts() {
            let mut rows = Vec::new();
            for (asset, denom) in [(Asset::Base, "DEVBASE"), (Asset::Quote, "DEVQUOTE")] {
                let b = self.ledger.balance(&a.owner, asset)?;
                rows.push(
                    json!({"denom":denom, "C":b.c.to_string(), "R":b.r.to_string(),
                    "D":b.d.to_string(), "P":b.p.to_string(), "A":b.available()?.to_string()}),
                );
            }
            accounts.push(json!({"owner":a.owner, "owner_epoch":a.epoch.to_string(),
                "ledger":rows, "withdraw_frozen":self.is_frozen(&a.owner)}));
        }
        let corrected = self.corrected_quantities()?;
        let mut ordered: Vec<_> = self.orders.values().collect();
        ordered.sort_by_key(|o| o.live.admission_seq);
        let mut orders = Vec::new();
        for o in ordered {
            let (raw, sig) = self
                .evidence(
                    "ORDER",
                    &o.live.owner,
                    &format!("{}:{}", o.epoch, o.order_id),
                )
                .ok_or("LEDGER_RECONCILIATION")?;
            let wire = Codec::default().decode("OrderV1", raw)?;
            let maximum = integer(&wire["max_qty_lots"], 64)? as u64;
            let cancelled = maximum
                .checked_sub(o.filled)
                .and_then(|n| n.checked_sub(o.live.remaining))
                .ok_or("LEDGER_RECONCILIATION")?;
            orders.push(json!({"owner":o.live.owner, "order_wire":STANDARD.encode(raw),
                "signature":STANDARD.encode(sig), "view":{
                "order_id":o.order_id, "order_hash":o.live.hash, "owner_epoch":o.epoch.to_string(),
                "admission_seq":o.live.admission_seq.to_string(),
                "side":if o.live.side == Side::Buy {"BUY"} else {"SELL"},
                "order_type":if wire["order_type"] == "1" {"LIMIT_GTC"} else {"LIMIT_IOC"},
                "limit_price_ticks":o.live.price.to_string(), "max_qty_lots":maximum.to_string(),
                "remaining_qty_lots":o.live.remaining.to_string(), "filled_qty_lots":o.filled.to_string(),
                "corrected_qty_lots":corrected.get(&o.live.hash).copied().unwrap_or(0).to_string(),
                "pending_qty_lots":self.ledger.quantities(&o.live.hash)?.pending.to_string(),
                "settled_qty_lots":self.ledger.quantities(&o.live.hash)?.settled.to_string(),
                "cancelled_qty_lots":cancelled.to_string(), "state":o.status, "revision":o.revision.to_string()
            }}));
        }
        let mut bindings = Vec::new();
        for ((kind, owner, _), b) in &self.bindings {
            let v = Codec::default().decode(
                if kind == "ORDER" {
                    "OrderV1"
                } else {
                    "CancelV1"
                },
                &b.raw,
            )?;
            let id = &v[if kind == "ORDER" {
                "order_id"
            } else {
                "cancel_nonce"
            }];
            let epoch = integer(&v["owner_epoch"], 64)? as u64;
            let raw_owner = STANDARD.decode(owner).map_err(|_| "ADDRESS_MISMATCH")?;
            bindings.push((
                (
                    raw_owner,
                    epoch,
                    id.as_str().unwrap().to_owned(),
                    kind.clone(),
                ),
                json!({"owner":owner, "owner_epoch":epoch.to_string(), "kind":kind, "id":id,
                    "request_hash":b.hash, "first_command_seq":b.outcome.seq.to_string()}),
            ));
        }
        for ((kind, owner, id), bound) in &self.local_bindings {
            let raw_owner = STANDARD.decode(owner).map_err(|_| "ADDRESS_MISMATCH")?;
            bindings.push((
                (raw_owner, bound.epoch, id.clone(), kind.clone()),
                json!({"owner":owner, "owner_epoch":bound.epoch.to_string(), "kind":kind,
                    "id":id, "request_hash":bound.outcome.hash,
                    "first_command_seq":bound.outcome.seq.to_string()}),
            ));
        }
        bindings.sort_by(|a, b| a.0.cmp(&b.0));
        let fills: Vec<_> = self
            .fill_order
            .iter()
            .map(|id| self.outbox.get(id).cloned().ok_or("LEDGER_RECONCILIATION"))
            .collect::<Result<_>>()?;
        let value = json!({"context":self.snapshot.value()["context"],
            "last_command_seq":self.seq.to_string(), "chain_snapshot":self.snapshot.value(), "mode":mode,
            "accounts":accounts, "orders":orders, "fills":fills,
            "bindings":bindings.into_iter().map(|(_,v)|v).collect::<Vec<_>>(),
            "batches":self.batches, "attempt_refs":self.attempts.iter().map(|a| super::engine::reference(a,"application/json")).collect::<Result<Vec<_>>>()?,
            "dependencies":self.fill_order.iter().map(|id| self.outbox[id]["dependency"].clone()).collect::<Vec<_>>(),
            "resolution_receipts":self.resolutions, "applied_batches":self.applied, "corrections":self.corrections,
            "latest_observation_ref":if self.observations.is_empty() {Value::Null} else {super::engine::reference(&json!(self.observations.iter().map(|s| s.value()).collect::<Vec<_>>()),"application/json")?},
            "stream_seq":self.seq.to_string()});
        schema::validate("EngineState", &value)?;
        canonical(&value).map_err(|_| "STATE_CANONICAL")?;
        Ok(value)
    }
    pub fn state_hash(&self, mode: &str) -> Result<String> {
        Ok(sha256(&codec::frame(
            "NUS/S3/ENGINE_STATE/V1",
            &canonical(&self.state_json(mode)?).map_err(|_| "STATE_CANONICAL")?,
        )))
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn is_frozen(&self, owner: &str) -> bool {
        self.frozen.contains_key(owner)
    }
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
        next.revise_orders(self)?;
        Ok((next, if hold { "UNSETTLED_HOLD" } else { "OK" }))
    }
    pub fn abort_withdraw(&self, owner: &str, observation: &Observation, now: u64) -> Result<Self> {
        let height = self.frozen.get(owner).ok_or("WITHDRAW_NOT_PREPARED")?;
        if self.snapshot.height() <= *height || self.mode() != "OPEN" {
            return Err("STALE");
        }
        self.snapshot.freshness(observation, now)?;
        self.ledger.validate()?;
        let mut next = self.clone();
        next.seq = next.seq.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
        next.frozen.remove(owner);
        Ok(next)
    }
    /// Session-authenticated local action. The API must derive session_owner
    /// from authentication, never from a client-supplied owner field. Raw input
    /// is canonical LocalAction JSON, preserving bytes for future journal replay.
    /// The returned candidate is private and is NOT a durable acknowledgement.
    pub fn local_action(
        &self,
        kind: &str,
        raw: &[u8],
        session_owner: &str,
        observation: &Observation,
        now: u64,
    ) -> Result<(Self, Outcome, bool)> {
        if !matches!(kind, "WITHDRAW_PREPARE" | "WITHDRAW_ABORT") {
            return Err("UNSUPPORTED_VERSION");
        }
        // LocalAction has one fixed-width field. Bound before parse/allocation.
        if raw.len() > 81 {
            return Err("LOCAL_ACTION_FORMAT");
        }
        let value: Value = serde_json::from_slice(raw).map_err(|_| "LOCAL_ACTION_FORMAT")?;
        let object = value.as_object().ok_or("LOCAL_ACTION_FORMAT")?;
        let id = value["request_id"].as_str().ok_or("LOCAL_ACTION_FORMAT")?;
        if object.len() != 1
            || id.len() != 64
            || !id
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || canonical(&value).map_err(|_| "LOCAL_ACTION_FORMAT")? != raw
        {
            return Err("LOCAL_ACTION_FORMAT");
        }
        let account = self
            .snapshot
            .accounts()
            .iter()
            .find(|a| a.owner == session_owner)
            .ok_or("ACCOUNT_KEY_UNREGISTERED")?;
        let key = (kind.to_owned(), session_owner.to_owned(), id.to_owned());
        let hash = sha256(raw);
        if let Some(bound) = self.local_bindings.get(&key) {
            if bound.raw != raw || bound.outcome.hash != hash {
                return Err("ID_CONFLICT");
            }
            return Ok((self.clone(), bound.outcome.clone(), true));
        }
        let (next, code) = if kind == "WITHDRAW_PREPARE" {
            self.prepare_withdraw(session_owner)?
        } else {
            match self.abort_withdraw(session_owner, observation, now) {
                Ok(next) => (next, "OK"),
                Err(code @ ("WITHDRAW_NOT_PREPARED" | "STALE")) => {
                    let mut next = self.clone();
                    next.seq = next.seq.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
                    (next, code)
                }
                Err(code) => return Err(code),
            }
        };
        let mut next = next;
        let outcome = Outcome {
            seq: next.seq,
            hash,
            code: code.into(),
            fills: vec![],
        };
        next.local_bindings.insert(
            key,
            LocalBound {
                epoch: account.epoch,
                outcome: outcome.clone(),
                raw: raw.to_vec(),
            },
        );
        Ok((next, outcome, false))
    }
    pub fn local_evidence(&self, kind: &str, owner: &str, id: &str) -> Option<&[u8]> {
        self.local_bindings
            .get(&(kind.into(), owner.into(), id.into()))
            .map(|bound| bound.raw.as_slice())
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
        let context = &self.snapshot.value()["context"];
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
        let bps = self.snapshot.bps();
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
                if !(20..=1000).contains(&delta) {
                    return Err("EXPIRY_MARGIN");
                }
                if self.history.iter().any(|s| {
                    s.value()["owner_events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|e| e["kind"] == "REVOKE_ORDER" && e["order_hash"] == hash)
                }) {
                    return Err("ORDER_REVOKED");
                }
                policy::market_rules(&v)?;
                if u128::from(bps) > integer(&v["max_fee_bps"], 32)? {
                    return Err("FEE_CAP");
                }
                let q = integer(&v["max_qty_lots"], 64)? as u64;
                let p = integer(&v["limit_price_ticks"], 64)? as u64;
                policy::fill(q, p, bps)?;
                self.snapshot.freshness(observation, now)?;
                if self.mode() != "OPEN" {
                    return Err("CATCHING_UP");
                }
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
                "ORDER_REVOKED",
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
        next.revise_orders(self)?;
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
            let identity = json!({"chain_id":self.snapshot.value()["context"]["chain_id"], "market_id":"DEVBASE/DEVQUOTE", "operator_epoch":self.snapshot.operator_epoch().to_string(), "command_seq":self.seq.to_string(), "match_index":index.to_string()});
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
            let f = self.ledger.fill(&fill_id).ok_or("LEDGER_RECONCILIATION")?;
            let buy_epoch = if buy == hash {
                integer(&v["owner_epoch"], 64)? as u64
            } else {
                self.orders[buy].epoch
            };
            let sell_epoch = if sell == hash {
                integer(&v["owner_epoch"], 64)? as u64
            } else {
                self.orders[sell].epoch
            };
            let node = self.graph.append(Identity {
                fill_id: fill_id.clone(),
                command_seq: self.seq,
                match_index: index as u32,
                orders: [buy.into(), sell.into()],
                debits: [
                    DebitDomain {
                        owner: f.buyer.clone(),
                        epoch: buy_epoch,
                        asset: DebitAsset::Quote,
                    },
                    DebitDomain {
                        owner: f.seller.clone(),
                        epoch: sell_epoch,
                        asset: DebitAsset::Base,
                    },
                ],
            })?;
            let dependency = json!({"fill_id":fill_id,"predecessor_fill_ids":node.predecessors,
                "order_hashes":[buy,sell],"buyer_owner":f.buyer,"buyer_epoch":buy_epoch.to_string(),
                "seller_owner":f.seller,"seller_epoch":sell_epoch.to_string(),"source_snapshot_id":self.snapshot.id()});
            self.outbox.insert(fill_id.clone(), json!({
                "fill_id":fill_id, "maker_order_hash":if side == Side::Buy {sell} else {buy},
                "taker_order_hash":hash, "buyer_order_hash":buy, "seller_order_hash":sell,
                "command_seq":self.seq.to_string(), "match_index":index.to_string(),
                "quantity_lots":f.quantity.to_string(), "execution_price_ticks":f.price.to_string(),
                "fee_policy_version":if bps==0 {"1"} else {"2"},
                "fee_base_atoms":f.base_fee.to_string(), "fee_quote_atoms":f.quote_fee.to_string(),
                "buy_D":f.buy_debit.to_string(), "sell_D":f.sell_debit.to_string(),
                "buyer_P":f.base_net.to_string(), "seller_P":f.quote_net.to_string(),
                "snapshot_id":self.snapshot.id(), "state":"PENDING", "revision":"1",
                "reason":"", "export_state":"QUEUED_S3", "submission_enabled":true,
                "origin_operator_epoch":self.snapshot.operator_epoch().to_string(), "batch":null,
                "dependency":dependency
            }));
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
            revision: 1,
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

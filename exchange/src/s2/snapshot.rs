//! Validated, immutable chain observations for sequencer candidate transitions.
//! Only a trusted local chain adapter may supply snapshots; this is not an HTTP
//! command or a consensus proof verifier. Callers must journal the complete
//! correction/ledger transition before replacing their committed snapshot.
use super::journal::{canonical, sha256};
use crate::{Result, codec};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

const DENOMS: [&str; 2] = ["DEVBASE", "DEVQUOTE"];
fn shape(v: &Value, keys: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or("SNAPSHOT_SCHEMA")?;
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err("SNAPSHOT_SCHEMA");
    }
    Ok(())
}
fn text(v: &Value) -> Result<&str> {
    v.as_str().ok_or("SNAPSHOT_SCHEMA")
}
fn hash(v: &Value) -> Result<&str> {
    let s = text(v)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("SNAPSHOT_HASH");
    }
    Ok(s)
}
fn bytes(v: &Value, size: usize) -> Result<Vec<u8>> {
    let s = text(v)?;
    let b = STANDARD.decode(s).map_err(|_| "SNAPSHOT_BYTES")?;
    if b.len() != size || STANDARD.encode(&b) != s {
        return Err("SNAPSHOT_BYTES");
    }
    Ok(b)
}
fn u64(v: &Value) -> Result<u64> {
    Ok(codec::integer(v, 64)? as u64)
}
fn pair(v: &Value) -> Result<&Vec<Value>> {
    v.as_array()
        .filter(|a| a.len() == 2)
        .ok_or("SNAPSHOT_SCHEMA")
}
fn add(a: u128, b: u128) -> Result<u128> {
    a.checked_add(b).ok_or("INTEGER_OVERFLOW")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub owner: String,
    pub public_key: Vec<u8>,
    pub epoch: u64,
    pub account_number: u64,
    pub sequence: u64,
    pub confirmed: [u128; 2],
    pub bank: [u128; 2],
}
/// Expected values come from the run manifest and registered genesis, never the
/// submitted snapshot. Test fixtures explicitly supply their synthetic context.
#[derive(Clone, Debug)]
pub struct Binding {
    context: Value,
    market: Value,
    owners: [Vec<u8>; 2],
    supplies: [u128; 2],
}
impl Binding {
    pub fn new(
        context: Value,
        market: Value,
        owners: [Vec<u8>; 2],
        supplies: [u128; 2],
    ) -> Result<Self> {
        shape(
            &context,
            &[
                "schema_version",
                "chain_id",
                "genesis_hash",
                "contract_hash",
                "config_hash",
                "market_id",
                "market_config_version",
            ],
        )?;
        for key in ["genesis_hash", "contract_hash", "config_hash"] {
            hash(&context[key])?;
        }
        let chain = text(&context["chain_id"])?;
        if chain.is_empty()
            || chain.len() > 128
            || !chain
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
            || context["schema_version"] != "1"
            || context["market_id"] != "DEVBASE/DEVQUOTE"
            || context["market_config_version"] != "1"
            || owners[0].len() != 20
            || owners[1].len() != 20
            || owners[0] >= owners[1]
        {
            return Err("CONTEXT_MISMATCH");
        }
        shape(
            &market,
            &[
                "market_id",
                "config_version",
                "base_denom",
                "quote_denom",
                "base_atoms_per_lot",
                "quote_atoms_per_lot_tick",
                "min_qty_lots",
                "max_qty_lots",
                "min_price_ticks",
                "max_price_ticks",
                "max_order_quote_atoms",
                "fee_policy_version",
                "fee_bps",
            ],
        )?;
        for (key, expected) in [
            ("market_id", "DEVBASE/DEVQUOTE"),
            ("config_version", "1"),
            ("base_denom", "DEVBASE"),
            ("quote_denom", "DEVQUOTE"),
            ("base_atoms_per_lot", "1000"),
            ("quote_atoms_per_lot_tick", "1"),
            ("min_qty_lots", "1"),
            ("max_qty_lots", "1000000"),
            ("min_price_ticks", "1"),
            ("max_price_ticks", "1000000"),
            ("max_order_quote_atoms", "1000000000000"),
        ] {
            if market[key] != expected {
                return Err("CONTEXT_MISMATCH");
            }
        }
        if !((market["fee_policy_version"] == "1" && market["fee_bps"] == "0")
            || (market["fee_policy_version"] == "2" && market["fee_bps"] == "25"))
        {
            return Err("CONTEXT_MISMATCH");
        }
        Ok(Self {
            context,
            market,
            owners,
            supplies,
        })
    }
    /// Parses unique keys before hashing. Enforces exact object fields, primitive
    /// ranges, registered owner ordering, key/address binding and conservation.
    pub fn decode(&self, raw: &[u8]) -> Result<Snapshot> {
        if raw.len() > 65536 {
            return Err("RESOURCE_LIMIT");
        }
        let value = codec::unique_json(raw)?;
        shape(&value, &["snapshot_id", "body"])?;
        let id = hash(&value["snapshot_id"])?;
        let body = &value["body"];
        shape(
            body,
            &[
                "context",
                "observed_height",
                "block_hash",
                "block_time_unix_ms",
                "market",
                "accounts",
                "supplies",
            ],
        )?;
        if body["context"] != self.context || body["market"] != self.market {
            return Err("CONTEXT_MISMATCH");
        }
        hash(&body["block_hash"])?;
        let height = u64(&body["observed_height"])?;
        let block_time = u64(&body["block_time_unix_ms"])?;
        let canonical = canonical(body).map_err(|_| "SNAPSHOT_SCHEMA")?;
        if sha256(&codec::frame("NUS/S2/SNAPSHOT/V1", &canonical)) != id {
            return Err("SNAPSHOT_HASH");
        }
        let mut accounts = Vec::new();
        for (i, a) in pair(&body["accounts"])?.iter().enumerate() {
            shape(
                a,
                &[
                    "owner",
                    "public_key_type",
                    "public_key",
                    "account_number",
                    "sequence",
                    "owner_epoch",
                    "gas_atoms",
                    "balances",
                ],
            )?;
            let owner = bytes(&a["owner"], 20)?;
            let key = bytes(&a["public_key"], 1952)?;
            if a["public_key_type"] != "ML_DSA_65"
                || owner != self.owners[i]
                || owner != codec::address(&key)?
            {
                return Err("ACCOUNT_KEY_MISMATCH");
            }
            let account_number = u64(&a["account_number"])?;
            let sequence = u64(&a["sequence"])?;
            codec::integer(&a["gas_atoms"], 128)?;
            let mut bank = [0; 2];
            let mut confirmed = [0; 2];
            for (j, balance) in pair(&a["balances"])?.iter().enumerate() {
                shape(balance, &["denom", "bank_atoms", "confirmed_atoms"])?;
                if balance["denom"] != DENOMS[j] {
                    return Err("SNAPSHOT_SCHEMA");
                }
                bank[j] = codec::integer(&balance["bank_atoms"], 128)?;
                confirmed[j] = codec::integer(&balance["confirmed_atoms"], 128)?;
            }
            accounts.push(Account {
                owner: text(&a["owner"])?.into(),
                public_key: key,
                epoch: u64(&a["owner_epoch"])?,
                account_number,
                sequence,
                confirmed,
                bank,
            });
        }
        if accounts[0].account_number == accounts[1].account_number {
            return Err("SNAPSHOT_CONFLICT");
        }
        for (i, supply) in pair(&body["supplies"])?.iter().enumerate() {
            shape(
                supply,
                &[
                    "denom",
                    "module_atoms",
                    "bank_supply_atoms",
                    "genesis_supply_atoms",
                ],
            )?;
            let module = codec::integer(&supply["module_atoms"], 128)?;
            let bank_supply = codec::integer(&supply["bank_supply_atoms"], 128)?;
            let genesis_supply = codec::integer(&supply["genesis_supply_atoms"], 128)?;
            let c = add(accounts[0].confirmed[i], accounts[1].confirmed[i])?;
            let bank = add(accounts[0].bank[i], accounts[1].bank[i])?;
            if supply["denom"] != DENOMS[i]
                || c != module
                || add(bank, module)? != bank_supply
                || bank_supply != genesis_supply
                || genesis_supply != self.supplies[i]
            {
                return Err("ASSET_CONSERVATION");
            }
        }
        Ok(Snapshot {
            value,
            accounts,
            height,
            block_time,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    value: Value,
    accounts: Vec<Account>,
    height: u64,
    block_time: u64,
}
impl Snapshot {
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn id(&self) -> &str {
        self.value["snapshot_id"].as_str().unwrap()
    }
    pub fn height(&self) -> u64 {
        self.height
    }
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }
    /// No mutation: the sequencer must apply connected-component correction and
    /// validate the complete candidate ledger before journaling this snapshot.
    pub fn advance(&self, next: &Self) -> Result<Advance> {
        if next.value["body"]["context"] != self.value["body"]["context"]
            || next.value["body"]["market"] != self.value["body"]["market"]
        {
            return Err("CONTEXT_MISMATCH");
        }
        if next.height < self.height {
            return Err("HEIGHT_REGRESSION");
        }
        if next.height == self.height {
            return if next.id() == self.id() {
                Ok(Advance::Duplicate)
            } else {
                Err("SNAPSHOT_CONFLICT")
            };
        }
        if self.height.checked_add(1) != Some(next.height) {
            return Err("CATCHING_UP");
        }
        if next.block_time < self.block_time {
            return Err("SNAPSHOT_CONFLICT");
        }
        for i in 0..2 {
            if next.value["body"]["supplies"][i]["genesis_supply_atoms"]
                != self.value["body"]["supplies"][i]["genesis_supply_atoms"]
            {
                return Err("ASSET_CONSERVATION");
            }
        }
        let mut changed = Vec::new();
        for (old, new) in self.accounts.iter().zip(&next.accounts) {
            if old.owner != new.owner || old.public_key != new.public_key {
                return Err("ACCOUNT_KEY_MISMATCH");
            }
            if new.account_number != old.account_number || new.sequence < old.sequence {
                return Err("SNAPSHOT_CONFLICT");
            }
            if new.epoch < old.epoch {
                return Err("EPOCH_MISMATCH");
            }
            if new.epoch == old.epoch && (0..2).any(|i| new.confirmed[i] < old.confirmed[i]) {
                return Err("UNPROVEN_BALANCE_DECREASE");
            }
            if new.epoch > old.epoch {
                changed.push(new.owner.clone());
            }
        }
        Ok(Advance::Next {
            epoch_changed_owners: changed,
        })
    }
    /// Derived from recorded adapter times, not client-supplied `fresh=true`.
    /// Replay passes the recorded decision time; it never consults wall clock.
    pub fn freshness(&self, observation: &Observation, now: u64) -> Result<()> {
        if observation.snapshot_id != self.id() || observation.cursor_height != self.height {
            return Err("SNAPSHOT_CONFLICT");
        }
        if observation.catching_up {
            return Err("CATCHING_UP");
        }
        if observation.query_latency_ms > 2000
            || observation.received_at > now
            || now - observation.received_at > 5000
            || self.block_time.saturating_sub(now) > 1000
            || now.saturating_sub(self.block_time) > 5000
        {
            return Err("STALE");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Advance {
    Duplicate,
    Next { epoch_changed_owners: Vec<String> },
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub snapshot_id: String,
    pub cursor_height: u64,
    pub received_at: u64,
    pub query_latency_ms: u64,
    pub catching_up: bool,
}

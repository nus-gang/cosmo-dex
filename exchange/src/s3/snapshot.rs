//! Same-height, manifest-bound chain observations. Only trusted local RPC may
//! supply these; this module is not a light client or an unauthenticated API.
use super::schema::{self, bytes, num};
pub use crate::s2::snapshot::{Account, Observation};
use crate::{Result, codec};
use serde_json::Value;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    context: Value,
    owners: Vec<Vec<u8>>,
    supplies: [u128; 2],
    bps: u32,
}
impl Binding {
    pub fn new(
        context: Value,
        owners: Vec<Vec<u8>>,
        supplies: [u128; 2],
        bps: u32,
    ) -> Result<Self> {
        schema::validate("Context", &context)?;
        if context["market_config_version"] != "1"
            || !(2..=16).contains(&owners.len())
            || owners.iter().any(|o| o.len() != 20)
            || owners.windows(2).any(|p| p[0] >= p[1])
            || ![0, 25].contains(&bps)
        {
            return Err("CONTEXT_MISMATCH");
        }
        Ok(Self {
            context,
            owners,
            supplies,
            bps,
        })
    }
    pub fn context(&self) -> &Value {
        &self.context
    }
    pub fn decode(&self, raw: &[u8]) -> Result<Snapshot> {
        let value = schema::decode("ChainSnapshot", raw)?;
        if value["context"] != self.context
            || bytes(&value["operator"])?.len() != 20
            || num(&value["operator_epoch"])? == 0
            || num(&value["height"])? == 0
        {
            return Err("CONTEXT_MISMATCH");
        }
        let mut body = value.clone();
        body.as_object_mut().unwrap().remove("snapshot_id");
        if schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", &body)? != value["snapshot_id"] {
            return Err("SNAPSHOT_HASH");
        }
        let rows = value["accounts"].as_array().unwrap();
        if rows.len() != self.owners.len() || value["assets"].as_array().unwrap().len() != 2 {
            return Err("SNAPSHOT_OWNER_SET");
        }
        let mut accounts = Vec::new();
        let mut sums = [0u128; 2];
        for (row, owner) in rows.iter().zip(&self.owners) {
            let pk = bytes(&row["public_key"])?;
            if bytes(&row["owner"])? != *owner
                || pk.len() != 1952
                || hex::decode(super::journal::sha256(&pk)).map_err(|_| "KEY_LENGTH")?[..20]
                    != owner[..]
            {
                return Err("ACCOUNT_KEY_MISMATCH");
            }
            let a = row["assets"].as_array().unwrap();
            if a.len() != 2 {
                return Err("SNAPSHOT_OWNER_SET");
            }
            let mut c = [0; 2];
            let mut bank = [0; 2];
            for (i, denom) in ["DEVBASE", "DEVQUOTE"].iter().enumerate() {
                if a[i]["denom"] != *denom {
                    return Err("SNAPSHOT_OWNER_SET");
                }
                c[i] = codec::integer(&a[i]["confirmed_atoms"], 128)?;
                bank[i] = codec::integer(&a[i]["bank_atoms"], 128)?;
                sums[i] = sums[i].checked_add(c[i]).ok_or("INTEGER_OVERFLOW")?;
            }
            accounts.push(Account {
                owner: row["owner"].as_str().unwrap().into(),
                public_key: pk,
                epoch: num(&row["epoch"])?,
                account_number: num(&row["account_number"])?,
                sequence: num(&row["sequence"])?,
                confirmed: c,
                bank,
            });
        }
        for (i, denom) in ["DEVBASE", "DEVQUOTE"].iter().enumerate() {
            let a = &value["assets"][i];
            let module = codec::integer(&a["module_bank_atoms"], 128)?;
            let treasury = codec::integer(&a["treasury_atoms"], 128)?;
            let unassigned = codec::integer(&a["unassigned_atoms"], 128)?;
            if a["denom"] != *denom
                || codec::integer(&a["sum_confirmed_atoms"], 128)? != sums[i]
                || sums[i]
                    .checked_add(treasury)
                    .and_then(|n| n.checked_add(unassigned))
                    != Some(module)
                || codec::integer(&a["supply_atoms"], 128)? != self.supplies[i]
            {
                return Err("ASSET_CONSERVATION");
            }
            // Manifest owns the full synthetic user set; no omitted bank owner.
            if accounts.iter().try_fold(module, |n, a| {
                n.checked_add(a.bank[i]).ok_or("INTEGER_OVERFLOW")
            })? != self.supplies[i]
            {
                return Err("ASSET_CONSERVATION");
            }
        }
        let last = num(&value["last_batch_seq"])?;
        if (last == 0) != (value["last_batch_hash"] == schema::ZERO) {
            return Err("RECEIPT_INCONSISTENCY");
        }
        let mut prior = 0;
        for seq in value["terminal_batch_seqs"].as_array().unwrap() {
            let seq = num(seq)?;
            if seq <= prior || seq > last {
                return Err("RECEIPT_INCONSISTENCY");
            }
            prior = seq;
        }
        let mut prior = None;
        for e in value["owner_events"].as_array().unwrap() {
            let ix = num(&e["tx_index"])?;
            if prior.is_some_and(|p| p > ix) || !accounts.iter().any(|a| a.owner == e["owner"]) {
                return Err("OWNER_EVENT");
            }
            prior = Some(ix);
            let before = num(&e["before_epoch"])?;
            let after = num(&e["after_epoch"])?;
            if (e["kind"] == "REVOKE_ORDER" && (before != after || e["order_hash"].is_null()))
                || (e["kind"] != "REVOKE_ORDER"
                    && (before.checked_add(1) != Some(after) || !e["order_hash"].is_null()))
            {
                return Err("OWNER_EVENT");
            }
        }
        Ok(Snapshot {
            binding: self.clone(),
            value,
            accounts,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    binding: Binding,
    value: Value,
    accounts: Vec<Account>,
}
impl Snapshot {
    pub fn decode_related(&self, raw: &[u8]) -> Result<Self> {
        self.binding.decode(raw)
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn context(&self) -> &Value {
        &self.binding.context
    }
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }
    pub fn height(&self) -> u64 {
        num(&self.value["height"]).unwrap()
    }
    pub fn id(&self) -> &str {
        self.value["snapshot_id"].as_str().unwrap()
    }
    pub fn bps(&self) -> u32 {
        self.binding.bps
    }
    pub fn operator_epoch(&self) -> u64 {
        num(&self.value["operator_epoch"]).unwrap()
    }
    pub fn last_seq(&self) -> u64 {
        num(&self.value["last_batch_seq"]).unwrap()
    }
    /// Validate one contiguous observation, including events that explain epochs.
    pub fn advance(&self, next: &Self) -> Result<bool> {
        if self.binding != next.binding {
            return Err("CONTEXT_MISMATCH");
        }
        if self.height() == next.height() {
            return if self.value == next.value {
                Ok(false)
            } else {
                Err("SNAPSHOT_CONFLICT")
            };
        }
        if self.height().checked_add(1) != Some(next.height()) {
            return Err("CATCHING_UP");
        }
        if num(&next.value["block_time_unix_ms"])? < num(&self.value["block_time_unix_ms"])?
            || next.operator_epoch() < self.operator_epoch()
            || next.last_seq() < self.last_seq()
        {
            return Err("SNAPSHOT_CONFLICT");
        }
        for (old, new) in self.accounts.iter().zip(&next.accounts) {
            if old.owner != new.owner
                || old.public_key != new.public_key
                || old.account_number != new.account_number
                || new.sequence < old.sequence
            {
                return Err("ACCOUNT_KEY_MISMATCH");
            }
            let mut epoch = old.epoch;
            for e in next.value["owner_events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["owner"] == old.owner)
            {
                if num(&e["before_epoch"])? != epoch {
                    return Err("OWNER_EVENT");
                }
                epoch = num(&e["after_epoch"])?;
            }
            if epoch != new.epoch {
                return Err("UNPROVEN_EPOCH_CHANGE");
            }
        }
        let slots: Vec<_> = next.value["terminal_batch_seqs"]
            .as_array()
            .unwrap()
            .iter()
            .map(num)
            .collect::<Result<_>>()?;
        let expected_count = next
            .last_seq()
            .checked_sub(self.last_seq())
            .ok_or("RECEIPT_INCONSISTENCY")?;
        if expected_count != slots.len() as u64
            || slots
                .iter()
                .enumerate()
                .any(|(i, s)| self.last_seq().checked_add(i as u64 + 1) != Some(*s))
            || (expected_count == 0
                && next.value["last_batch_hash"] != self.value["last_batch_hash"])
        {
            return Err("RECEIPT_INCONSISTENCY");
        }
        Ok(true)
    }
    pub fn freshness(&self, o: &Observation, now: u64) -> Result<()> {
        if o.snapshot_id != self.id() || o.cursor_height != self.height() {
            return Err("SNAPSHOT_CONFLICT");
        }
        if o.catching_up {
            return Err("CATCHING_UP");
        }
        let time = num(&self.value["block_time_unix_ms"])?;
        if o.query_latency_ms > 2000
            || o.received_at > now
            || now - o.received_at > 5000
            || time.saturating_sub(now) > 1000
            || now.saturating_sub(time) > 5000
        {
            return Err("STALE");
        }
        Ok(())
    }
}

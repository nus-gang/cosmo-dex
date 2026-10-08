//! Approved account overlay. Only Engine can produce a marker/replay-verified
//! ReceiptSource; public bytes never substitute for the trusted result ledger.
use super::{Error, Result};
use crate::{
    codec,
    s3::{
        journal::{MAX_PAYLOAD, canonical, sha256},
        schema,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};
pub const PUBLIC_RECEIPT_VERSION: &str = "s3-dev-local-account/1";
pub const PUBLIC_RECEIPT_SCHEMA_SHA256: &str =
    "2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b";
const LISTS: [&str; 5] = [
    "affected_order_hashes",
    "created_fill_ids",
    "corrected_fill_ids",
    "committed_fill_ids",
    "applied_batch_ids",
];
const USER_KINDS: [&str; 4] = ["ORDER", "CANCEL", "WITHDRAW_PREPARE", "WITHDRAW_ABORT"];
static PUBLIC_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../../proposals/s3-local-account-receipt-v1/schema.json"
    ))
    .expect("approved schema")
});
fn invalid() -> Error {
    Error::Recovery("RECEIPT_SOURCE_INVALID")
}
fn require(ok: bool) -> Result<()> {
    if ok { Ok(()) } else { Err(invalid()) }
}
fn rows(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(invalid)
}
fn string(v: &Value) -> Result<&str> {
    v.as_str().ok_or_else(invalid)
}
#[derive(Clone, Debug)]
pub struct AccountReceipt {
    raw: Vec<u8>,
}
impl AccountReceipt {
    /// Exact canonical UTF-8 JSON body; no newline, compression or wrapper.
    pub fn as_bytes(&self) -> &[u8] {
        &self.raw
    }
    pub fn to_value(&self) -> Value {
        serde_json::from_slice(&self.raw).expect("validated receipt")
    }
}
#[derive(Clone, Debug)]
pub struct ReceiptSource {
    frame: Vec<u8>,
    record: Value,
    result: Value,
    state: Value,
}
impl ReceiptSource {
    /// Privileged raw evidence. Never expose this through public REST.
    pub fn frame(&self) -> &[u8] {
        &self.frame
    }
    pub fn result(&self) -> &Value {
        &self.result
    }
    pub fn state(&self) -> &Value {
        &self.state
    }
    pub fn project(&self, principal: &str) -> Result<Option<AccountReceipt>> {
        let principal_value = json!(principal);
        require(schema::bytes(&principal_value)?.len() == 20)?;
        let accounts = rows(&self.state["accounts"])?;
        let owners: BTreeSet<_> = accounts
            .iter()
            .map(|a| string(&a["owner"]))
            .collect::<Result<_>>()?;
        require(owners.len() == accounts.len())?;
        if !owners.contains(principal) {
            return Ok(None);
        }
        let order_rows = rows(&self.state["orders"])?;
        let orders: BTreeMap<_, _> = order_rows
            .iter()
            .map(|o| Ok((string(&o["view"]["order_hash"])?, o)))
            .collect::<Result<_>>()?;
        require(orders.len() == order_rows.len())?;
        let own_orders: BTreeSet<_> = orders
            .iter()
            .filter(|(_, o)| o["owner"] == principal)
            .map(|(h, _)| *h)
            .collect();
        let mut fills = BTreeSet::new();
        let mut batches = BTreeSet::new();
        let mut own_fills = BTreeSet::new();
        let mut own_batches = BTreeSet::new();
        for fill in rows(&self.state["fills"])? {
            let id = string(&fill["fill_id"])?;
            require(fills.insert(id))?;
            let buyer = string(&fill["buyer_order_hash"])?;
            let seller = string(&fill["seller_order_hash"])?;
            require(orders.contains_key(buyer) && orders.contains_key(seller))?;
            let own = own_orders.contains(buyer) || own_orders.contains(seller);
            if own {
                own_fills.insert(id);
            }
            if !fill["batch"].is_null() {
                let id = string(&fill["batch"]["batch_id"])?;
                batches.insert(id);
                if own {
                    own_batches.insert(id);
                }
            }
        }
        let all_orders: BTreeSet<_> = orders.keys().copied().collect();
        let result = &self.result;
        let mut r = serde_json::Map::new();
        for k in [
            "kind",
            "request_hash",
            "code",
            "state",
            "observed_height",
            "snapshot_id",
        ] {
            r.insert(k.into(), result[k].clone());
        }
        for name in LISTS {
            let (known, own) = match name {
                "affected_order_hashes" => (&all_orders, &own_orders),
                "applied_batch_ids" => (&batches, &own_batches),
                _ => (&fills, &own_fills),
            };
            let mut seen = BTreeSet::new();
            let mut projected = Vec::new();
            for id in rows(&result[name])? {
                let h = string(id)?;
                require(seen.insert(h) && known.contains(h))?;
                if own.contains(h) {
                    projected.push(id.clone());
                }
            }
            r.insert(name.into(), json!(projected));
        }
        r.insert(
            "ledger_changes".into(),
            json!(
                rows(&result["ledger_changes"])?
                    .iter()
                    .filter(|row| row["owner"] == principal)
                    .cloned()
                    .collect::<Vec<_>>()
            ),
        );
        if USER_KINDS.contains(&string(&result["kind"])?) {
            let bindings: Vec<_> = rows(&self.state["bindings"])?
                .iter()
                .filter(|b| {
                    b["kind"] == result["kind"]
                        && b["first_command_seq"] == result["command_seq"]
                        && b["request_hash"] == result["request_hash"]
                })
                .collect();
            require(bindings.len() == 1)?;
            if bindings[0]["owner"] != principal {
                return Ok(None);
            }
        } else if !LISTS
            .iter()
            .any(|k| r[*k].as_array().is_some_and(|a| !a.is_empty()))
            && rows(&r["ledger_changes"])?.is_empty()
        {
            return Ok(None);
        }
        let out = json!({"envelope_version":PUBLIC_RECEIPT_VERSION,"profile_id":"s3-dev-local-v1","context":self.record["context"],"principal":principal,"development_receipt":"LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE","durable_ack":false,"storage_assurance":"UNPROVEN_HOST_SPACE","source":{"command_seq":result["command_seq"],"record_hash":sha256(&self.frame),"command_result_hash":self.record["result_hash"],"after_state_hash":result["after_state_hash"]},"account_result":r});
        validate(&out, &PUBLIC_SCHEMA)?;
        require(schema::num(&out["source"]["command_seq"])? > 0)?;
        require((r["code"] == "OK") == (r["state"] == "LOCAL_ACCEPTED"))?;
        let mut denoms = BTreeSet::new();
        let mut last = "";
        for row in rows(&r["ledger_changes"])? {
            let denom = string(&row["after"]["denom"])?;
            require(
                row["before"]["denom"] == row["after"]["denom"]
                    && denoms.insert(denom)
                    && last < denom,
            )?;
            last = denom;
            for side in ["before", "after"] {
                let x = &row[side];
                let c = codec::integer(&x["C"], 128)?;
                let reserved = codec::integer(&x["R"], 128)?;
                let d = codec::integer(&x["D"], 128)?;
                require(
                    c.checked_sub(reserved).and_then(|v| v.checked_sub(d))
                        == Some(codec::integer(&x["A"], 128)?),
                )?;
            }
        }
        let raw = canonical(&out)?;
        require(raw.len() <= MAX_PAYLOAD)?;
        Ok(Some(AccountReceipt { raw }))
    }
    pub(super) fn check_admission(&self) -> Result<()> {
        // Bound inherited base64 result + <=4096 byte envelope overhead before
        // admission; check each exact projection too. No page truncation.
        schema::validate("CommandResult", &self.result)?;
        let n = canonical(&self.result)?.len();
        require(n <= 12_582_912 && n.checked_add(4096).is_some_and(|n| n <= MAX_PAYLOAD))?;
        for a in rows(&self.state["accounts"])? {
            self.project(string(&a["owner"])?)?;
        }
        Ok(())
    }
}
// Called only on a private Prepared candidate (admission) or during full
// store-backed semantic replay. Never exposed as a source constructor.
pub(super) fn source(frame: Vec<u8>, record: &Value) -> Result<ReceiptSource> {
    require(frame.len() >= 72 && frame.len() <= MAX_PAYLOAD + 72)?;
    require(super::store::frame(&canonical(record)?)? == frame)?;
    let raw_result = schema::bytes(&record["result_json"])?;
    let raw_state = schema::bytes(&record["state_json"])?;
    let result = schema::decode("CommandResult", &raw_result)?;
    let state = schema::decode("EngineState", &raw_state)?;
    require(canonical(&result)? == raw_result && canonical(&state)? == raw_state)?;
    require(
        result["command_seq"] == record["command_seq"]
            && result["command_seq"] == state["last_command_seq"]
            && result["kind"] == record["command_kind"],
    )?;
    require(
        record["context"] == state["context"] && record["context"] == record["snapshot"]["context"],
    )?;
    require(
        record["after_state_hash"] == result["after_state_hash"]
            && record["after_state_hash"] == schema::hash("NUS/S3/ENGINE_STATE/V1", &state)?
            && record["result_hash"] == schema::hash("NUS/S3/COMMAND_RESULT/V1", &result)?,
    )?;
    require(
        result["snapshot_id"] == record["snapshot"]["snapshot_id"]
            && result["observed_height"] == record["snapshot"]["height"],
    )?;
    Ok(ReceiptSource {
        frame,
        record: record.clone(),
        result,
        state,
    })
}
fn validate(v: &Value, s: &Value) -> Result<()> {
    if let Some(r) = s["$ref"].as_str() {
        let name = r.strip_prefix("#/$defs/").ok_or_else(invalid)?;
        match name {
            "U64" => {
                schema::num(v)?;
            }
            "Atoms" => {
                codec::integer(v, 128)?;
            }
            "Hash" => {
                schema::validate("Hash", v)?;
            }
            "Owner" => {
                require(schema::bytes(v)?.len() == 20)?;
            }
            _ => {}
        }
        return validate(v, &PUBLIC_SCHEMA["$defs"][name]);
    }
    require(
        s.get("const").is_none_or(|c| c == v) && s["enum"].as_array().is_none_or(|a| a.contains(v)),
    )?;
    match s["type"].as_str() {
        Some("object") => {
            let o = v.as_object().ok_or_else(invalid)?;
            let keys = rows(&s["required"])?;
            require(o.len() == keys.len())?;
            for k in keys {
                let k = string(k)?;
                validate(o.get(k).ok_or_else(invalid)?, &s["properties"][k])?;
            }
        }
        Some("array") => {
            let a = rows(v)?;
            require(s["maxItems"].as_u64().is_none_or(|n| a.len() as u64 <= n))?;
            for v in a {
                validate(v, &s["items"])?;
            }
        }
        Some("string") => {
            let x = string(v)?;
            require(
                s["maxLength"].as_u64().is_none_or(|n| x.len() as u64 <= n)
                    && s["minLength"].as_u64().is_none_or(|n| x.len() as u64 >= n),
            )?;
        }
        Some("boolean") => require(v.is_boolean())?,
        _ => return Err(invalid()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("proposals/s3-local-account-receipt-v1/vectors")
    }
    #[test]
    fn approved_oracle_vectors_match_exact_bytes() {
        let root = fixture();
        let index: Value =
            serde_json::from_slice(&std::fs::read(root.join("index.json")).unwrap()).unwrap();
        for case in index["cases"].as_array().unwrap() {
            let raw = std::fs::read(root.join(case["source"].as_str().unwrap())).unwrap();
            let record: Value = serde_json::from_slice(&raw[72..]).unwrap();
            let source = source(raw, &record).unwrap();
            let actual = source
                .project(case["principal"].as_str().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(
                actual.as_bytes(),
                std::fs::read(root.join(case["expected"].as_str().unwrap())).unwrap(),
                "{}",
                case["id"]
            );
            source.check_admission().unwrap();
        }
    }
    #[test]
    fn projection_caps_and_invalid_source_are_not_truncated_or_hidden() {
        let root = fixture();
        let raw = std::fs::read(root.join("fee0-taker-trade.frame")).unwrap();
        let record: Value = serde_json::from_slice(&raw[72..]).unwrap();
        assert!(source(vec![0; MAX_PAYLOAD + 73], &record).is_err());
        let original = source(raw, &record).unwrap();
        let expected: Value =
            serde_json::from_slice(&std::fs::read(root.join("fee0-taker-trade.json")).unwrap())
                .unwrap();
        let owner = expected["principal"].as_str().unwrap();
        for mode in 0..5 {
            let mut s = original.clone();
            match mode {
                0 => s.result["code"] = json!("UNREGISTERED"),
                1 => s.state["orders"] = json!([]),
                2 => {
                    let x = s.state["fills"][0].clone();
                    s.state["fills"].as_array_mut().unwrap().push(x);
                }
                3 => {
                    s.result["ledger_changes"][0]["after"]["A"] =
                        json!("340282366920938463463374607431768211456")
                }
                _ => s.result["created_fill_ids"] = json!([schema::ZERO]),
            }
            assert!(s.check_admission().is_err(), "{mode}");
        }
        let mut s = original;
        let mut fills = vec![];
        let mut ids = vec![];
        for i in 0..1001 {
            let id = format!("{i:064x}");
            let mut f = s.state["fills"][0].clone();
            f["fill_id"] = json!(id);
            fills.push(f);
            ids.push(id);
        }
        s.state["fills"] = json!(fills);
        s.result["created_fill_ids"] = json!(ids);
        assert_eq!(
            s.project(owner).unwrap().unwrap().to_value()["account_result"]["created_fill_ids"]
                .as_array()
                .unwrap()
                .len(),
            1001
        );
        let mut v = expected;
        v["account_result"]["created_fill_ids"] = json!(vec!["a".repeat(64); 250407]);
        assert!(validate(&v, &PUBLIC_SCHEMA).is_err());
        s.result["request_hash"] = json!("x".repeat(12_582_913));
        assert!(s.check_admission().is_err());
    }
}

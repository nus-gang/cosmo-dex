//! Checked-integer port of the approved rc3 conservative capacity oracle.
//! A certificate is a bound, never proof that disk blocks have been reserved.
use super::{
    evidence::Objects,
    journal::canonical,
    schema::{self, SCHEMA},
};
use crate::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;
const LIMIT: u128 = 16_777_216;
fn add(a: u128, b: u128) -> Result<u128> {
    a.checked_add(b).ok_or("STORAGE_CAPACITY")
}
fn mul(a: u128, b: u128) -> Result<u128> {
    a.checked_mul(b).ok_or("STORAGE_CAPACITY")
}
fn len(v: &Value) -> Result<u128> {
    Ok(canonical(v).map_err(|_| "S3_CANONICAL")?.len() as u128)
}
fn array(n: u128, item: u128) -> Result<u128> {
    add(add(2, mul(n, item)?)?, n.saturating_sub(1))
}
fn object(fields: &BTreeMap<String, u128>) -> Result<u128> {
    let mut size = add(2, (fields.len() as u128).saturating_sub(1))?;
    for (k, v) in fields {
        size = add(size, add(add(len(&json!(k))?, 1)?, *v)?)?;
    }
    Ok(size)
}
fn b64(n: u128) -> Result<u128> {
    mul(4, add(n, 2)? / 3)
}
fn alloc(n: u128) -> Result<u128> {
    add(mul(add(n, 4095)? / 4096, 4096)?, 8192)
}
pub struct Bounds {
    counts: BTreeMap<String, u128>,
}
impl Bounds {
    pub fn new(pending: u128, orders: u128) -> Self {
        let mut counts = BTreeMap::new();
        for prefix in ["Correction", "CorrectionRecord"] {
            for field in ["root_fill_ids", "corrected_fill_ids", "surviving_fill_ids"] {
                counts.insert(format!("{prefix}.{field}"), pending);
            }
            for field in ["affected_order_hashes", "cancelled_order_hashes"] {
                counts.insert(format!("{prefix}.{field}"), orders);
            }
        }
        for field in [
            "created_fill_ids",
            "corrected_fill_ids",
            "committed_fill_ids",
            "applied_batch_ids",
            "correction_results",
        ] {
            counts.insert(format!("CommandResult.{field}"), pending);
        }
        counts.insert("CommandResult.affected_order_hashes".into(), orders);
        counts.insert("EngineAccount.ledger".into(), 2);
        // Overflow remains fail-closed when the field is evaluated.
        if let Some(n) = pending.checked_add(orders) {
            counts.insert("JournalRecord.external_event_ids".into(), n);
        }
        Self { counts }
    }
    pub fn size(&self, name: &str) -> Result<u128> {
        self.spec(SCHEMA["$defs"].get(name).ok_or("UNBOUNDED_TYPE")?, name)
    }
    fn spec(&self, s: &Value, path: &str) -> Result<u128> {
        if let Some(r) = s["$ref"].as_str() {
            return self.size(r.strip_prefix("#/$defs/").ok_or("UNBOUNDED_TYPE")?);
        }
        if let Some(a) = s["anyOf"].as_array() {
            return a
                .iter()
                .map(|v| self.spec(v, path))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .max()
                .ok_or("UNBOUNDED_TYPE");
        }
        if let Some(v) = s.get("const") {
            return len(v);
        }
        if let Some(a) = s["enum"].as_array() {
            return a
                .iter()
                .map(len)
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .max()
                .ok_or("UNBOUNDED_TYPE");
        }
        match s["type"].as_str() {
            Some("object") => object(
                &s["properties"]
                    .as_object()
                    .ok_or("UNBOUNDED_TYPE")?
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), self.spec(v, &format!("{path}.{k}"))?)))
                    .collect::<Result<_>>()?,
            ),
            Some("array") => {
                let count = s["maxItems"]
                    .as_u64()
                    .map(u128::from)
                    .or_else(|| self.counts.get(path).copied())
                    .ok_or("UNBOUNDED_ARRAY")?;
                array(count, self.spec(&s["items"], &format!("{path}[]"))?)
            }
            Some("boolean") => Ok(5),
            Some("null") => Ok(4),
            Some("string") => {
                if s["pattern"] == "^[0-9a-f]{64}$" {
                    return Ok(66);
                }
                let m = s["maxLength"].as_u64().ok_or("UNBOUNDED_STRING")? as u128;
                let factor = match s["pattern"].as_str() {
                    Some(
                        "^(0|[1-9][0-9]*)$"
                        | "^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$",
                    ) => 1,
                    _ => 12,
                };
                add(2, mul(factor, m)?)
            }
            _ => Err("UNBOUNDED_TYPE"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate {
    pub pending_fills: u128,
    pub orders: u128,
    pub max_state_bytes: u128,
    pub max_result_bytes: u128,
    pub max_journal_payload_bytes: u128,
    pub max_drain_records: u128,
    pub max_new_evidence_objects: u128,
    pub evidence_reserved_bytes: u128,
    pub reserved_bytes: u128,
}
impl Certificate {
    pub fn admissible(&self) -> bool {
        self.max_journal_payload_bytes <= LIMIT
    }
    /// Additional dedicated allocation required before ACK. Passing this number
    /// to statvfs or writing a releasable reserve file does not satisfy rc3.
    pub fn additional(&self, prior_reserved: u128) -> Result<u128> {
        if !self.admissible() {
            return Err("STORAGE_CAPACITY");
        }
        Ok(self.reserved_bytes.saturating_sub(prior_reserved))
    }
    pub fn value(&self) -> Value {
        json!({"pending_fills":self.pending_fills.to_string(),"orders":self.orders.to_string(),"max_state_bytes":self.max_state_bytes.to_string(),"max_result_bytes":self.max_result_bytes.to_string(),"max_journal_payload_bytes":self.max_journal_payload_bytes.to_string(),"max_drain_records":self.max_drain_records.to_string(),"max_new_evidence_objects":self.max_new_evidence_objects.to_string(),"evidence_reserved_bytes":self.evidence_reserved_bytes.to_string(),"reserved_bytes":self.reserved_bytes.to_string(),"admissible":self.admissible()})
    }
}
pub fn certificate(state: &Value, objects: &Objects) -> Result<Certificate> {
    schema::validate("EngineState", state)?;
    let n = state["fills"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["state"] == "PENDING" || f["state"] == "SUBMISSION_UNKNOWN")
        .count() as u128;
    let o = state["orders"].as_array().unwrap().len() as u128;
    let b = Bounds::new(n, o);
    let mut fields = BTreeMap::new();
    for (key, spec) in SCHEMA["$defs"]["EngineState"]["properties"]
        .as_object()
        .unwrap()
    {
        let size = match key.as_str() {
            "bindings"
            | "dependencies"
            | "corrections"
            | "resolution_receipts"
            | "applied_batches"
            | "attempt_refs" => {
                let items = state[key].as_array().unwrap();
                let append = match key.as_str() {
                    "bindings" | "dependencies" => 0,
                    "attempt_refs" => mul(25, n)?,
                    _ => n,
                };
                let sum = items.iter().try_fold(0, |a, v| add(a, len(v)?))?;
                let count = add(items.len() as u128, append)?;
                add(
                    add(add(2, sum)?, mul(append, b.spec(&spec["items"], key)?)?)?,
                    count.saturating_sub(1),
                )?
            }
            "orders" => {
                let mut size = add(2, o.saturating_sub(1))?;
                for order in state[key].as_array().unwrap() {
                    let f = order
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| {
                            Ok((
                                k.clone(),
                                if ["owner", "order_wire", "signature"].contains(&k.as_str()) {
                                    len(v)?
                                } else {
                                    b.size("OrderView")?
                                },
                            ))
                        })
                        .collect::<Result<_>>()?;
                    size = add(size, object(&f)?)?;
                }
                size
            }
            "fills" | "batches" => array(
                add(
                    state[key].as_array().unwrap().len() as u128,
                    if key == "batches" { n } else { 0 },
                )?,
                b.spec(&spec["items"], key)?,
            )?,
            _ => b.spec(spec, &format!("EngineState.{key}"))?,
        };
        fields.insert(key.clone(), size);
    }
    let smax = object(&fields)?;
    let rmax = b.size("CommandResult")?;
    let retained = objects.graph(state)?.len() as u128;
    let future = add(add(mul(181, n)?, o)?, 2)?;
    let mut journal = BTreeMap::new();
    for (key, spec) in SCHEMA["$defs"]["JournalRecord"]["properties"]
        .as_object()
        .unwrap()
    {
        let size = match key.as_str() {
            "state_json" => add(2, b64(smax)?)?,
            "result_json" => add(2, b64(rmax)?)?,
            "evidence_refs" => array(add(retained, future)?, b.size("EvidenceRef")?)?,
            _ => b.spec(spec, &format!("JournalRecord.{key}"))?,
        };
        journal.insert(key.clone(), size);
    }
    let jmax = object(&journal)?;
    let q = add(add(mul(40, n)?, o)?, 2)?;
    let per_record = add(
        add(
            add(alloc(add(72, jmax)?)?, mul(2, alloc(smax)?)?)?,
            mul(2, alloc(rmax)?)?,
        )?,
        mul(2, alloc(4096)?)?,
    )?;
    let per_fill = add(
        add(mul(80, alloc(16_777_216)?)?, mul(5, alloc(139_264)?)?)?,
        mul(96, alloc(262_144)?)?,
    )?;
    let evidence = add(mul(n, per_fill)?, mul(add(o, 2)?, alloc(262_144)?)?)?;
    Ok(Certificate {
        pending_fills: n,
        orders: o,
        max_state_bytes: smax,
        max_result_bytes: rmax,
        max_journal_payload_bytes: jmax,
        max_drain_records: q,
        max_new_evidence_objects: future,
        evidence_reserved_bytes: evidence,
        reserved_bytes: add(mul(q, per_record)?, evidence)?,
    })
}

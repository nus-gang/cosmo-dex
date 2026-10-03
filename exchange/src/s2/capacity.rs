//! Conservative *encoded* correction bound, not a row-count limit. The envelope
//! preserves all historical signed orders/bindings/outbox and includes every
//! pending fill and live remainder of both owners. It is never an executable
//! state: schema maxima deliberately over-approximate reachable combinations.
use super::{
    journal::{self, canonical},
    sequencer::Candidate,
};
use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::LazyLock;
static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../protocol/s2/schema.json")).expect("pinned schema")
});

/// Keep variable-length immutable bytes (signed wire/key) exactly. Every mutable
/// integer uses its full decimal width; Text is 128 safe ASCII bytes. Enums use
/// the longest JSON encoding and bool uses false (5 bytes). Base64 is applied
/// only AFTER inflating the embedded state/result, preserving its rounding.
fn inflate(value: &mut Value, schema: &Value) -> Result<()> {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference
            .strip_prefix("#/$defs/")
            .ok_or("CAPACITY_SCHEMA")?;
        match name {
            "U32" => *value = json!(u32::MAX.to_string()),
            "U64" => *value = json!(u64::MAX.to_string()),
            "Atoms" => *value = json!(u128::MAX.to_string()),
            "Text" => *value = json!("Z".repeat(128)),
            "Bytes" | "Hash" => {}
            _ => inflate(value, &SCHEMA["$defs"][name])?,
        }
        return Ok(());
    }
    if let Some(choices) = schema["enum"].as_array() {
        *value = choices
            .iter()
            .max_by_key(|v| v.to_string().len())
            .ok_or("CAPACITY_SCHEMA")?
            .clone();
    } else {
        match schema["type"].as_str() {
            Some("object") => {
                for (key, child) in value.as_object_mut().ok_or("CAPACITY_SCHEMA")? {
                    inflate(child, &schema["properties"][key])?;
                }
            }
            Some("array") => {
                for child in value.as_array_mut().ok_or("CAPACITY_SCHEMA")? {
                    inflate(child, &schema["items"])?;
                }
            }
            Some("boolean") => *value = json!(false),
            _ => return Err("CAPACITY_SCHEMA"),
        }
    }
    Ok(())
}
pub fn maximum_correction_payload(candidate: &Candidate, mode: &str) -> Result<usize> {
    let state = candidate.state_json(mode)?;
    maximum_for_state(&state)
}
/// Kept crate-private so an external caller cannot substitute a truncated state.
pub(crate) fn maximum_for_state(state: &Value) -> Result<usize> {
    let mut after = state.clone();
    let pending: Vec<_> = state["fills"]
        .as_array()
        .ok_or("CAPACITY_SCHEMA")?
        .iter()
        .filter(|f| f["state"] == "PENDING")
        .collect();
    let affected: Vec<_> = state["orders"]
        .as_array()
        .ok_or("CAPACITY_SCHEMA")?
        .iter()
        .filter(|o| {
            o["view"]["remaining_qty_lots"] != "0"
                || pending.iter().any(|f| {
                    f["buyer_order_hash"] == o["view"]["order_hash"]
                        || f["seller_order_hash"] == o["view"]["order_hash"]
                })
        })
        .map(|o| o["view"]["order_hash"].clone())
        .collect();
    let mut changes = Vec::new();
    for a in state["accounts"].as_array().ok_or("CAPACITY_SCHEMA")? {
        for row in a["ledger"].as_array().ok_or("CAPACITY_SCHEMA")? {
            changes.push(json!({"owner":a["owner"], "before":row, "after":row}));
        }
    }
    let mut result = json!({"command_seq":"0", "kind":"CORRECTION", "request_hash":"0".repeat(64),
        "code":"OK", "state":"LOCAL_ACCEPTED", "observed_height":"0", "snapshot_id":"0".repeat(64),
        "affected_order_hashes":affected, "created_fill_ids":[],
        "corrected_fill_ids":pending.iter().map(|f| f["fill_id"].clone()).collect::<Vec<_>>(),
        "ledger_changes":changes, "after_state_hash":"0".repeat(64)});
    inflate(&mut after, &SCHEMA["$defs"]["EngineState"])?;
    inflate(&mut result, &SCHEMA["$defs"]["CommandResult"])?;
    let mut record = json!({"context":after["context"], "command_seq":"0", "previous_commit_hash":"0".repeat(64),
        "command_kind":"CORRECTION", "recorded_at_unix_ms":"0", "request_wire":"", "signature":"",
        "signature_hash":"0".repeat(64), "snapshot":after["chain_snapshot"],
        "observation":{"snapshot_id":"0".repeat(64), "observed_height":"0", "cursor_height":"0",
            "received_at_unix_ms":"0", "block_age_ms":"0", "query_latency_ms":"0", "last_success_age_ms":"0",
            "catching_up":false, "fresh":false}, "before_state_hash":"0".repeat(64), "after_state_hash":"0".repeat(64),
        "result_json":STANDARD.encode(canonical(&result).map_err(|_| "CAPACITY_SCHEMA")?),
        "state_json":STANDARD.encode(canonical(&after).map_err(|_| "CAPACITY_SCHEMA")?),
        "result_hash":"0".repeat(64), "external_event_ids":["0".repeat(64)]});
    inflate(&mut record, &SCHEMA["$defs"]["JournalRecord"])?;
    Ok(canonical(&record).map_err(|_| "CAPACITY_SCHEMA")?.len())
}
pub fn check(record: &Value, candidate: &Candidate, mode: &str) -> journal::Result<usize> {
    let maximum =
        maximum_correction_payload(candidate, mode).map_err(journal::Error::InvalidRecord)?;
    if maximum > journal::MAX_PAYLOAD || canonical(record)?.len() > journal::MAX_PAYLOAD {
        return Err(journal::Error::ResourceLimit);
    }
    Ok(maximum)
}

//! Contract result/record projection for private signed-command candidates.
//! This does not own admission, correction capacity or recovery. The service must
//! retain the candidate privately until journal append succeeds, and index the
//! original record/receipt for retries rather than projecting a duplicate again.
use super::{
    journal::{self, Commit, canonical, sha256},
    sequencer::{Candidate, Outcome},
    snapshot::Observation,
};
use crate::{
    Result,
    codec::{self, Codec},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub struct SignedRecord {
    record: Value,
    result: Value,
    owner: Value,
    epoch: Value,
    request_id: Value,
}
impl SignedRecord {
    /// Only a first execution, with adjacent before/after states, can be recorded.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        before: &Candidate,
        after: &Candidate,
        outcome: &Outcome,
        kind: &str,
        observation: &Observation,
        now: u64,
        mode: &str,
        previous: &Commit,
    ) -> Result<Self> {
        if after.sequence() != before.sequence().checked_add(1).ok_or("INTEGER_OVERFLOW")?
            || outcome.seq != after.sequence()
            || previous.command_seq != before.sequence()
        {
            return Err("RECORD_SEQUENCE");
        }
        let name = match kind {
            "ORDER" => "OrderV1",
            "CANCEL" => "CancelV1",
            _ => return Err("RECORD_KIND"),
        };
        let a = before.state_json(mode)?;
        let b = after.state_json(mode)?;
        // Signed commands cannot change the authoritative chain snapshot.
        if a["chain_snapshot"] != b["chain_snapshot"] {
            return Err("RECORD_SNAPSHOT");
        }
        let sequence_text = outcome.seq.to_string();
        let binding = b["bindings"]
            .as_array()
            .ok_or("RECORD_BINDING")?
            .iter()
            .find(|v| {
                v["first_command_seq"] == sequence_text
                    && v["kind"] == kind
                    && v["request_hash"] == outcome.hash
            })
            .ok_or("RECORD_BINDING")?;
        let owner = binding["owner"].as_str().ok_or("RECORD_BINDING")?;
        let id = if kind == "ORDER" {
            format!(
                "{}:{}",
                binding["owner_epoch"].as_str().unwrap(),
                binding["id"].as_str().unwrap()
            )
        } else {
            binding["id"].as_str().unwrap().to_owned()
        };
        let (raw, sig) = after.evidence(kind, owner, &id).ok_or("RECORD_BINDING")?;
        let wire = Codec::default().decode(name, raw)?;
        let mut changed = vec![];
        for account in b["accounts"].as_array().unwrap() {
            let old = a["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["owner"] == account["owner"])
                .ok_or("RECORD_ACCOUNT")?;
            for (old, new) in old["ledger"]
                .as_array()
                .unwrap()
                .iter()
                .zip(account["ledger"].as_array().unwrap())
            {
                if old != new {
                    changed.push(json!({"owner":account["owner"], "before":old, "after":new}));
                }
            }
        }
        let affected: Vec<_> = b["orders"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|order| {
                a["orders"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|old| old["view"]["order_hash"] == order["view"]["order_hash"])
                    != Some(*order)
            })
            .map(|order| order["view"]["order_hash"].clone())
            .collect();
        let created: Vec<_> = b["fills"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["command_seq"] == sequence_text)
            .map(|f| f["fill_id"].clone())
            .collect();
        if created != outcome.fills.iter().map(|s| json!(s)).collect::<Vec<_>>() {
            return Err("RECORD_FILLS");
        }
        let after_hash = after.state_hash(mode)?;
        let result = json!({"command_seq":outcome.seq.to_string(), "kind":kind,
            "request_hash":outcome.hash, "code":outcome.code,
            "state":if outcome.code == "OK" {"LOCAL_ACCEPTED"} else {"REJECTED"},
            "observed_height":after.snapshot().height().to_string(), "snapshot_id":after.snapshot().id(),
            "affected_order_hashes":affected, "created_fill_ids":created, "corrected_fill_ids":[],
            "ledger_changes":changed, "after_state_hash":after_hash});
        let result_bytes = canonical(&result).map_err(|_| "RECORD_CANONICAL")?;
        let state_bytes = canonical(&b).map_err(|_| "RECORD_CANONICAL")?;
        let block_time: u64 =
            codec::integer(&b["chain_snapshot"]["body"]["block_time_unix_ms"], 64)? as u64;
        let obs = json!({"snapshot_id":observation.snapshot_id,
            "observed_height":after.snapshot().height().to_string(), "cursor_height":observation.cursor_height.to_string(),
            "received_at_unix_ms":observation.received_at.to_string(), "block_age_ms":now.saturating_sub(block_time).to_string(),
            "query_latency_ms":observation.query_latency_ms.to_string(), "last_success_age_ms":now.saturating_sub(observation.received_at).to_string(),
            "catching_up":observation.catching_up, "fresh":after.snapshot().freshness(observation,now).is_ok()});
        let record = json!({"context":b["context"], "command_seq":outcome.seq.to_string(),
            "previous_commit_hash":previous.record_hash, "command_kind":kind, "recorded_at_unix_ms":now.to_string(),
            "request_wire":STANDARD.encode(raw), "signature":STANDARD.encode(sig), "signature_hash":sha256(sig),
            "snapshot":b["chain_snapshot"], "observation":obs, "before_state_hash":before.state_hash(mode)?,
            "after_state_hash":after_hash, "result_json":STANDARD.encode(&result_bytes), "state_json":STANDARD.encode(state_bytes),
            "result_hash":sha256(&codec::frame("NUS/S2/RESULT/V1", &result_bytes)), "external_event_ids":[]});
        Ok(Self {
            record,
            result,
            owner: wire["owner"].clone(),
            epoch: wire["owner_epoch"].clone(),
            request_id: wire[if kind == "ORDER" {
                "order_id"
            } else {
                "cancel_nonce"
            }]
            .clone(),
        })
    }
    pub fn record(&self) -> &Value {
        &self.record
    }
    pub fn result(&self) -> &Value {
        &self.result
    }
    /// Call only with the actual successful append result (or verified replay
    /// index). Matching the complete frame prevents pairing a receipt with a
    /// different record at the same sequence. This is not a durability proof.
    pub fn receipt(&self, commit: &Commit) -> Result<Value> {
        let bytes = canonical(&self.record).map_err(|_| "RECORD_CANONICAL")?;
        let hash = sha256(&journal::frame(&bytes).map_err(|_| "RESOURCE_LIMIT")?);
        let sequence_text = commit.command_seq.to_string();
        if self.record["command_seq"] != sequence_text || hash != commit.record_hash {
            return Err("RECEIPT_COMMIT_MISMATCH");
        }
        Ok(
            json!({"context":self.record["context"], "kind":self.record["command_kind"],
            "request_id":self.request_id, "request_hash":self.result["request_hash"], "owner":self.owner,
            "owner_epoch":self.epoch, "command_seq":self.result["command_seq"], "state":self.result["state"],
            "code":self.result["code"], "durability":"LOCAL_FSYNC", "replicated":false,
            "observed_height":self.result["observed_height"], "snapshot_id":self.result["snapshot_id"],
            "result_hash":self.record["result_hash"], "journal_commit_hash":commit.record_hash}),
        )
    }
}

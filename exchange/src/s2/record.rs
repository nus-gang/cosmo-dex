//! Contract result/record projection for private signed-command candidates.
//! This does not own admission, correction capacity or recovery. The service must
//! retain the candidate privately until journal append succeeds, and index the
//! original record/receipt for retries rather than projecting a duplicate again.
use super::{
    journal::{self, Commit, canonical, sha256},
    sequencer::{Candidate, Outcome},
    snapshot::{Advance, Observation, Snapshot},
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
        let (changed, affected) = changes(&a, &b)?;
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
    /// Re-execute a signed record from its recorded context. Never trust the
    /// embedded state/result as a recovery snapshot. The caller must first
    /// verify journal framing/commit markers and replay every preceding event.
    pub fn replay(
        before: &Candidate,
        record: &Value,
        mode: &str,
        previous: &Commit,
    ) -> Result<(Candidate, Self)> {
        let kind = record["command_kind"].as_str().ok_or("REPLAY_KIND")?;
        let name = match kind {
            "ORDER" => "OrderV1",
            "CANCEL" => "CancelV1",
            _ => return Err("REPLAY_KIND"),
        };
        let decode = |field: &str| {
            STANDARD
                .decode(record[field].as_str().ok_or("REPLAY_ENCODING")?)
                .map_err(|_| "REPLAY_ENCODING")
        };
        let raw = decode("request_wire")?;
        let sig = decode("signature")?;
        let wire = Codec::default().decode(name, &raw)?;
        let now = codec::integer(&record["recorded_at_unix_ms"], 64)? as u64;
        let obs = &record["observation"];
        let observation = Observation {
            snapshot_id: obs["snapshot_id"]
                .as_str()
                .ok_or("REPLAY_OBSERVATION")?
                .into(),
            cursor_height: codec::integer(&obs["cursor_height"], 64)? as u64,
            received_at: codec::integer(&obs["received_at_unix_ms"], 64)? as u64,
            query_latency_ms: codec::integer(&obs["query_latency_ms"], 64)? as u64,
            catching_up: obs["catching_up"].as_bool().ok_or("REPLAY_OBSERVATION")?,
        };
        let (after, outcome, duplicate) = before.submit(
            kind,
            &raw,
            &sig,
            wire["owner"].as_str().ok_or("REPLAY_OWNER")?,
            &observation,
            now,
        )?;
        if duplicate {
            return Err("REPLAY_DUPLICATE");
        }
        let prepared = Self::prepare(
            before,
            &after,
            &outcome,
            kind,
            &observation,
            now,
            mode,
            previous,
        )?;
        if prepared.record() != record {
            return Err("REPLAY_RECORD_MISMATCH");
        }
        Ok((after, prepared))
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

fn changes(a: &Value, b: &Value) -> Result<(Vec<Value>, Vec<Value>)> {
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
    Ok((changed, affected))
}

/// Trusted chain adapter input only. No signed-command endpoint accepts this.
/// The correction plan is derived from the prior committed state and new snapshot;
/// its hash is stored in CommandResult and the full plan is reproducible on replay.
pub struct SnapshotRecord {
    record: Value,
    result: Value,
    correction: Option<Value>,
}
impl SnapshotRecord {
    pub fn prepare(
        before: &Candidate,
        snapshot: Snapshot,
        observation: &Observation,
        now: u64,
        mode: &str,
        previous: &Commit,
    ) -> Result<(Candidate, Option<Self>)> {
        if previous.command_seq != before.sequence() {
            return Err("RECORD_SEQUENCE");
        }
        // A typed Snapshot may have been constructed with another manifest.
        let snapshot = before
            .snapshot()
            .decode_related(&canonical(snapshot.value()).map_err(|_| "RECORD_CANONICAL")?)?;
        let changed_owners = match before.snapshot().advance(&snapshot)? {
            Advance::Duplicate => return Ok((before.clone(), None)),
            Advance::Next {
                epoch_changed_owners,
            } => epoch_changed_owners,
        };
        if observation.snapshot_id != snapshot.id()
            || observation.cursor_height != snapshot.height()
        {
            return Err("RECORD_OBSERVATION");
        }
        let (after, correction) = before.advance(snapshot)?;
        let a = before.state_json(mode)?;
        let b = after.state_json(mode)?;
        let before_hash = before.state_hash(mode)?;
        let after_hash = after.state_hash(mode)?;
        // Use snapshot account order (raw owner bytes), not base64 lexical order.
        let affected_owners: Vec<_> = after
            .snapshot()
            .accounts()
            .iter()
            .filter(|a| correction.affected_owners.contains(&a.owner))
            .map(|a| a.owner.clone())
            .collect();
        let correction_json = if changed_owners.is_empty() {
            None
        } else {
            Some(json!({
                "snapshot_id":after.snapshot().id(), "changed_owners":changed_owners,
                "affected_owners":affected_owners, "cancelled_order_hashes":correction.cancelled_order_hashes,
                "corrected_fill_ids":correction.corrected_fill_ids, "reason":"OWNER_EPOCH_CHANGED",
                "before_state_hash":before_hash, "after_state_hash":after_hash
            }))
        };
        let kind = if correction_json.is_some() {
            "CORRECTION"
        } else {
            "SNAPSHOT"
        };
        let request_hash = match &correction_json {
            Some(c) => sha256(&canonical(c).map_err(|_| "RECORD_CANONICAL")?),
            None => after.snapshot().id().to_owned(),
        };
        let (changed, affected) = changes(&a, &b)?;
        let result = json!({"command_seq":after.sequence().to_string(), "kind":kind,
            "request_hash":request_hash, "code":"OK", "state":"LOCAL_ACCEPTED",
            "observed_height":after.snapshot().height().to_string(), "snapshot_id":after.snapshot().id(),
            "affected_order_hashes":affected, "created_fill_ids":[],
            "corrected_fill_ids":correction.corrected_fill_ids, "ledger_changes":changed,
            "after_state_hash":after_hash});
        let result_bytes = canonical(&result).map_err(|_| "RECORD_CANONICAL")?;
        let block_time =
            codec::integer(&b["chain_snapshot"]["body"]["block_time_unix_ms"], 64)? as u64;
        let obs = json!({"snapshot_id":observation.snapshot_id,
            "observed_height":after.snapshot().height().to_string(), "cursor_height":observation.cursor_height.to_string(),
            "received_at_unix_ms":observation.received_at.to_string(), "block_age_ms":now.saturating_sub(block_time).to_string(),
            "query_latency_ms":observation.query_latency_ms.to_string(), "last_success_age_ms":now.saturating_sub(observation.received_at).to_string(),
            "catching_up":observation.catching_up, "fresh":after.snapshot().freshness(observation,now).is_ok()});
        let record = json!({"context":b["context"], "command_seq":after.sequence().to_string(),
            "previous_commit_hash":previous.record_hash, "command_kind":kind, "recorded_at_unix_ms":now.to_string(),
            "request_wire":"", "signature":"", "signature_hash":sha256(&[]),
            "snapshot":b["chain_snapshot"], "observation":obs, "before_state_hash":before_hash,
            "after_state_hash":after_hash, "result_json":STANDARD.encode(&result_bytes),
            "state_json":STANDARD.encode(canonical(&b).map_err(|_| "RECORD_CANONICAL")?),
            "result_hash":sha256(&codec::frame("NUS/S2/RESULT/V1", &result_bytes)),
            "external_event_ids":[after.snapshot().id()]});
        Ok((
            after,
            Some(Self {
                record,
                result,
                correction: correction_json,
            }),
        ))
    }
    pub fn replay(
        before: &Candidate,
        record: &Value,
        mode: &str,
        previous: &Commit,
    ) -> Result<(Candidate, Self)> {
        let snapshot = before
            .snapshot()
            .decode_related(&canonical(&record["snapshot"]).map_err(|_| "RECORD_CANONICAL")?)?;
        let obs = &record["observation"];
        let observation = Observation {
            snapshot_id: obs["snapshot_id"]
                .as_str()
                .ok_or("REPLAY_OBSERVATION")?
                .into(),
            cursor_height: codec::integer(&obs["cursor_height"], 64)? as u64,
            received_at: codec::integer(&obs["received_at_unix_ms"], 64)? as u64,
            query_latency_ms: codec::integer(&obs["query_latency_ms"], 64)? as u64,
            catching_up: obs["catching_up"].as_bool().ok_or("REPLAY_OBSERVATION")?,
        };
        let now = codec::integer(&record["recorded_at_unix_ms"], 64)? as u64;
        let (after, prepared) = Self::prepare(before, snapshot, &observation, now, mode, previous)?;
        let prepared = prepared.ok_or("REPLAY_DUPLICATE")?;
        if prepared.record != *record {
            return Err("REPLAY_RECORD_MISMATCH");
        }
        Ok((after, prepared))
    }
    pub fn record(&self) -> &Value {
        &self.record
    }
    pub fn result(&self) -> &Value {
        &self.result
    }
    pub fn correction(&self) -> Option<&Value> {
        self.correction.as_ref()
    }
}

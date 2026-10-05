//! Complete rc3 record projection and deterministic semantic re-execution.
//! These are PRIVATE preparations, never ACKs. A supported dedicated allocator
//! and atomic journal publisher must wrap them before any service exposes state.
use super::{
    evidence::Objects,
    journal::{self, Commit, canonical, sha256},
    schema::{self, bytes, num},
    sequencer::Candidate,
    snapshot::Observation,
};
use crate::{Result, codec::Codec};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub struct Prepared {
    pub record: Value,
    pub result: Value,
}
fn changed(before: &Value, after: &Value) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut ledger = vec![];
    for account in after["accounts"].as_array().unwrap() {
        let old = before["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["owner"] == account["owner"])
            .ok_or("RECORD_ACCOUNT")?;
        for (a, b) in old["ledger"]
            .as_array()
            .unwrap()
            .iter()
            .zip(account["ledger"].as_array().unwrap())
        {
            if a != b {
                ledger.push(json!({"owner":account["owner"],"before":a,"after":b}));
            }
        }
    }
    let orders = after["orders"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| {
            before["orders"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["view"]["order_hash"] == o["view"]["order_hash"])
                != Some(*o)
        })
        .map(|o| o["view"]["order_hash"].clone())
        .collect();
    Ok((ledger, orders))
}
impl Prepared {
    pub fn prepare(
        before: &Candidate,
        after: &Candidate,
        kind: &str,
        observation: &Observation,
        now: u64,
        previous: &Commit,
    ) -> Result<Self> {
        if before.sequence().checked_add(1) != Some(after.sequence())
            || previous.command_seq != before.sequence()
        {
            return Err("RECORD_SEQUENCE");
        }
        let a = before.full_state()?;
        let b = after.full_state()?;
        let bh = before.full_hash()?;
        let ah = after.full_hash()?;
        let (ledger, mut affected) = changed(&a, &b)?;
        let old_c = a["corrections"].as_array().unwrap();
        let all_c = b["corrections"].as_array().unwrap();
        if !all_c.starts_with(old_c) {
            return Err("CORRECTION_IMMUTABLE");
        }
        let mut corrections = all_c[old_c.len()..].to_vec();
        if (kind == "CORRECTION") == corrections.is_empty() {
            return Err("RECORD_KIND");
        }
        let mut correction_ids = vec![];
        for c in &mut corrections {
            if c["before_state_hash"] != bh || num(&c["command_seq"])? != after.sequence() {
                return Err("CORRECTION_BINDING");
            }
            correction_ids.push(c["correction_id"].clone());
            c["after_state_hash"] = json!(ah);
            schema::validate("Correction", c)?;
        }
        let mut corrected = vec![];
        if !corrections.is_empty() {
            affected.clear();
            for c in &corrections {
                for v in c["affected_order_hashes"].as_array().unwrap() {
                    if !affected.contains(v) {
                        affected.push(v.clone());
                    }
                }
                for v in c["corrected_fill_ids"].as_array().unwrap() {
                    if !corrected.contains(v) {
                        corrected.push(v.clone());
                    }
                }
            }
        }
        let mut created = vec![];
        let mut committed = vec![];
        for f in b["fills"].as_array().unwrap() {
            let old = a["fills"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["fill_id"] == f["fill_id"]);
            if old.is_none() {
                created.push(f["fill_id"].clone());
            }
            if f["state"] == "COMMITTED" && old.is_some_and(|x| x["state"] != "COMMITTED") {
                committed.push(f["fill_id"].clone());
            }
        }
        let applied: Vec<_> = b["applied_batches"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| !a["applied_batches"].as_array().unwrap().contains(x))
            .map(|x| x["batch_id"].clone())
            .collect();
        let (raw, sig, request_hash, code) = if [
            "ORDER",
            "CANCEL",
            "WITHDRAW_PREPARE",
            "WITHDRAW_ABORT",
        ]
        .contains(&kind)
        {
            let super::sequencer::CommandEvidence {
                kind: k,
                raw,
                sig,
                outcome,
            } = after.latest_command().ok_or("RECORD_BINDING")?;
            if k != kind || outcome.seq != after.sequence() {
                return Err("RECORD_BINDING");
            }
            (raw, sig, outcome.hash, outcome.code)
        } else {
            let hash = if kind == "CORRECTION" {
                schema::hash(
                    "NUS/S3/CORRECTION_COMMAND/V1",
                    &json!({"context":b["context"],"command_seq":after.sequence().to_string(),"snapshot_id":after.snapshot().id(),"correction_ids":correction_ids}),
                )?
            } else {
                after.latest().id().to_owned()
            };
            (vec![], vec![], hash, "OK".into())
        };
        let result = json!({"command_seq":after.sequence().to_string(),"kind":kind,"request_hash":request_hash,"code":code,"state":if code=="OK"{"LOCAL_ACCEPTED"}else{"REJECTED"},"observed_height":after.latest().height().to_string(),"snapshot_id":after.latest().id(),"affected_order_hashes":affected,"created_fill_ids":created,"corrected_fill_ids":corrected,"ledger_changes":ledger,"after_state_hash":ah,"committed_fill_ids":committed,"applied_batch_ids":applied,"correction_results":corrections});
        schema::validate("CommandResult", &result)?;
        let time = num(&after.latest().value()["block_time_unix_ms"])?;
        let obs = json!({"snapshot_id":observation.snapshot_id,"observed_height":after.latest().height().to_string(),"cursor_height":observation.cursor_height.to_string(),"received_at_unix_ms":observation.received_at.to_string(),"block_age_ms":now.saturating_sub(time).to_string(),"query_latency_ms":observation.query_latency_ms.to_string(),"last_success_age_ms":now.saturating_sub(observation.received_at).to_string(),"catching_up":observation.catching_up,"fresh":after.latest().freshness(observation,now).is_ok()});
        let refs = after
            .evidence_set()?
            .graph(&json!([b, result, after.latest().value()]))?;
        let record = json!({"context":b["context"],"command_seq":after.sequence().to_string(),"previous_commit_hash":previous.record_hash,"command_kind":kind,"recorded_at_unix_ms":now.to_string(),"request_wire":STANDARD.encode(raw),"signature":STANDARD.encode(&sig),"signature_hash":sha256(&sig),"snapshot":after.latest().value(),"observation":obs,"before_state_hash":bh,"after_state_hash":ah,"result_json":STANDARD.encode(canonical(&result).map_err(|_|"S3_CANONICAL")?),"state_json":STANDARD.encode(canonical(&b).map_err(|_|"S3_CANONICAL")?),"result_hash":schema::hash("NUS/S3/COMMAND_RESULT/V1",&result)?,"external_event_ids":correction_ids,"evidence_refs":refs});
        schema::validate("JournalRecord", &record)?;
        journal::frame(&canonical(&record).map_err(|_| "S3_CANONICAL")?)
            .map_err(|_| "STORAGE_CAPACITY")?;
        Ok(Self { record, result })
    }
    /// Embedded state is only an input selector. It is never deserialized into
    /// balances/book. Execute the original transition then compare full bytes.
    pub fn replay(
        before: &Candidate,
        r: &Value,
        objects: &Objects,
        previous: &Commit,
    ) -> Result<(Candidate, Self)> {
        schema::validate("JournalRecord", r)?;
        let b_raw = bytes(&r["state_json"])?;
        let result_raw = bytes(&r["result_json"])?;
        let b = schema::decode("EngineState", &b_raw)?;
        let result = schema::decode("CommandResult", &result_raw)?;
        if canonical(&b).map_err(|_| "S3_CANONICAL")? != b_raw
            || canonical(&result).map_err(|_| "S3_CANONICAL")? != result_raw
        {
            return Err("REPLAY_CANONICAL");
        }
        objects.verify_exact_refs(&json!([b, result, r["snapshot"]]), &r["evidence_refs"])?;
        let mut input = before.clone();
        for (reference, raw) in objects.entries() {
            input.provide_evidence(raw, reference["media_type"].as_str().unwrap())?;
        }
        let kind = r["command_kind"].as_str().unwrap();
        let o = &r["observation"];
        let observation = Observation {
            snapshot_id: o["snapshot_id"]
                .as_str()
                .ok_or("REPLAY_OBSERVATION")?
                .into(),
            cursor_height: num(&o["cursor_height"])?,
            received_at: num(&o["received_at_unix_ms"])?,
            query_latency_ms: num(&o["query_latency_ms"])?,
            catching_up: o["catching_up"].as_bool().ok_or("REPLAY_OBSERVATION")?,
        };
        let now = num(&r["recorded_at_unix_ms"])?;
        let after = match kind {
            "ORDER" | "CANCEL" | "WITHDRAW_PREPARE" | "WITHDRAW_ABORT" => {
                let raw = bytes(&r["request_wire"])?;
                let sig = bytes(&r["signature"])?;
                let local = kind.starts_with("WITHDRAW_");
                let owner = if local {
                    b["bindings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|v| v["first_command_seq"] == r["command_seq"] && v["kind"] == kind)
                        .ok_or("REPLAY_BINDING")?["owner"]
                        .as_str()
                        .ok_or("REPLAY_BINDING")?
                        .to_owned()
                } else {
                    Codec::default().decode(
                        if kind == "ORDER" {
                            "OrderV1"
                        } else {
                            "CancelV1"
                        },
                        &raw,
                    )?["owner"]
                        .as_str()
                        .ok_or("REPLAY_BINDING")?
                        .into()
                };
                let (after, _, duplicate) = if local {
                    if !sig.is_empty() {
                        return Err("REPLAY_SIGNATURE");
                    }
                    input.local_action(kind, &raw, &owner, &observation, now)?
                } else {
                    input.submit(kind, &raw, &sig, &owner, &observation, now)?
                };
                if duplicate {
                    return Err("REPLAY_DUPLICATE");
                }
                after
            }
            "SNAPSHOT" => input.observe(
                input
                    .snapshot()
                    .decode_related(&canonical(&r["snapshot"]).map_err(|_| "S3_CANONICAL")?)?,
            )?,
            "SEAL_BATCH" => input.seal_batch(
                b["batches"]
                    .as_array()
                    .unwrap()
                    .last()
                    .ok_or("REPLAY_BATCH")?["seal_purpose"]
                    .as_str()
                    .ok_or("REPLAY_BATCH")?,
                &observation,
                now,
            )?,
            "RESOLVE_ATTEMPT"
                if b["resolution_receipts"].as_array().unwrap().len()
                    > before.full_state()?["resolution_receipts"]
                        .as_array()
                        .unwrap()
                        .len() =>
            {
                let old = before.full_state()?;
                let added: Vec<_> = b["resolution_receipts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| !old["resolution_receipts"].as_array().unwrap().contains(r))
                    .collect();
                if added.len() != 1 || added[0]["disposition"] != "COMMITTED" {
                    return Err("REPLAY_RECEIPTS");
                }
                input.record_receipt(added[0].clone())?
            }
            "ATTEMPT" | "RESOLVE_ATTEMPT" => {
                let candidates = b["attempt_refs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| objects.typed(r, "Attempt"))
                    .collect::<Result<Vec<_>>>()?;
                let delta: Vec<_> = candidates
                    .iter()
                    .filter(|a| !input.attempts().contains(a))
                    .collect();
                if delta.len() != 1 {
                    return Err("REPLAY_ATTEMPTS");
                }
                if kind == "ATTEMPT" {
                    input.prepare_attempt(delta[0].clone())?
                } else {
                    input.resolve_attempt(delta[0].clone())?
                }
            }
            "VOID_BATCH" => {
                let old = before.full_state()?;
                let added: Vec<_> = b["resolution_receipts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| !old["resolution_receipts"].as_array().unwrap().contains(r))
                    .collect();
                match added.len() {
                    0 => input.reject_final(input.rejection_evidence()?)?,
                    1 if added[0]["disposition"] == "VOID" => {
                        input.record_receipt(added[0].clone())?
                    }
                    _ => return Err("REPLAY_RECEIPTS"),
                }
            }
            "SETTLEMENT_APPLY" | "CORRECTION" => input.apply()?,
            _ => return Err("REPLAY_KIND"),
        };
        let prepared = Self::prepare(&input, &after, kind, &observation, now, previous)?;
        if prepared.record != *r {
            return Err("REPLAY_RECORD_MISMATCH");
        }
        Ok((after, prepared))
    }
}

//! S3 batch and reconciliation candidate transitions. Publication belongs only to
//! the serialized journal service; this module performs no network operations.
use super::{journal::canonical, schema};
use crate::Result;
use serde_json::{Value, json};
pub fn reference(v: &Value, media_type: &str) -> Result<Value> {
    let b = canonical(v).map_err(|_| "S3_CANONICAL")?;
    Ok(
        json!({"sha256":super::journal::sha256(&b),"byte_length":b.len().to_string(),"media_type":media_type}),
    )
}
use super::{
    dependencies::{Asset as DebitAsset, DebitDomain},
    ledger::Side,
    proof,
    schema::num,
    sequencer::Candidate,
    snapshot::{Observation, Snapshot},
    wire,
};
use crate::codec::Codec;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeSet;
fn bump(v: &mut Value) -> Result<()> {
    v["revision"] = json!(
        num(&v["revision"])?
            .checked_add(1)
            .ok_or("INTEGER_OVERFLOW")?
            .to_string()
    );
    Ok(())
}
impl Candidate {
    pub fn mode(&self) -> &str {
        if self
            .batches
            .iter()
            .any(|b| b["state"] == "RECOVERY_REQUIRED")
        {
            "RECOVERY_REQUIRED"
        } else if !self.observations.is_empty() {
            "CATCHING_UP"
        } else {
            "OPEN"
        }
    }
    pub fn full_state(&self) -> Result<Value> {
        self.state_json(self.mode())
    }
    pub fn full_hash(&self) -> Result<String> {
        self.state_hash(self.mode())
    }
    pub fn batches(&self) -> &[Value] {
        &self.batches
    }
    pub fn attempts(&self) -> &[Value] {
        &self.attempts
    }
    pub fn batch_wire(&self, id: &str) -> Option<&[u8]> {
        self.batch_wires.get(id).map(Vec::as_slice)
    }
    pub fn latest(&self) -> &Snapshot {
        self.history.last().unwrap()
    }
    fn history_refs(&self) -> Vec<&Snapshot> {
        self.history.iter().collect()
    }
    fn next(&self) -> Result<Self> {
        let mut n = self.clone();
        n.seq = n.seq.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
        Ok(n)
    }
    fn active(&self) -> Result<usize> {
        self.batches
            .iter()
            .position(|b| {
                !["COMMITTED", "CORRECTED"].contains(&b["state"].as_str().unwrap())
                    && !self.resolutions.iter().any(|r| r["batch"] == b["batch"])
            })
            .ok_or("BATCH_NOT_FOUND")
    }
    /// Persist an observation separately from applied C. Until apply succeeds,
    /// old C/R/D/P are frozen and admission remains closed, even after restart.
    pub fn observe(&self, s: Snapshot) -> Result<Self> {
        if !self.latest().advance(&s)? {
            return Ok(self.clone());
        }
        let mut n = self.next()?;
        n.history.push(s.clone());
        n.observations.push(s);
        Ok(n)
    }
    fn invalid_order(&self, hash: &str) -> bool {
        let o = &self.orders[hash];
        self.latest()
            .accounts()
            .iter()
            .find(|a| a.owner == o.live.owner)
            .is_none_or(|a| a.epoch != o.epoch)
            || self.history.iter().any(|s| {
                s.value()["owner_events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["kind"] == "REVOKE_ORDER" && e["order_hash"] == hash)
            })
    }
    /// Seal only the oldest unassigned FIFO prefix. No seq/identity allocation
    /// happens for the rest of the queue while this immutable batch is pending.
    pub fn seal_batch(&self, purpose: &str, o: &Observation, now: u64) -> Result<Self> {
        if self.active().is_ok() {
            return Err("BATCH_INFLIGHT");
        }
        if !["NORMAL", "RESOLVE_FAILURE"].contains(&purpose) {
            return Err("SEAL_PURPOSE");
        }
        self.latest().freshness(o, now)?;
        if purpose == "NORMAL" && self.mode() != "OPEN" {
            return Err("CATCHING_UP");
        }
        let ids: Vec<_> = self
            .fill_order
            .iter()
            .filter(|id| {
                self.outbox[*id]["export_state"] == "QUEUED_S3"
                    && self.graph.node(id).unwrap().state.pending()
            })
            .take(8)
            .cloned()
            .collect();
        if ids.is_empty() {
            return Err("EMPTY_BATCH");
        }
        let epoch = num(&self.outbox[&ids[0]]["origin_operator_epoch"])?;
        let ids: Vec<_> = ids
            .into_iter()
            .take_while(|id| num(&self.outbox[id]["origin_operator_epoch"]).ok() == Some(epoch))
            .collect();
        let mut order_hashes = BTreeSet::new();
        for id in &ids {
            let f = &self.outbox[id];
            for k in ["buyer_order_hash", "seller_order_hash"] {
                order_hashes.insert(f[k].as_str().unwrap().to_owned());
            }
        }
        let invalid = order_hashes.iter().any(|h| {
            self.invalid_order(h) || self.orders[h].live.expiry_height <= self.latest().height()
        }) || epoch != self.latest().operator_epoch();
        if purpose == "RESOLVE_FAILURE" && !invalid {
            return Err("SEAL_PURPOSE");
        }
        if purpose == "NORMAL"
            && (invalid
                || order_hashes.iter().any(|h| {
                    self.orders[h]
                        .live
                        .expiry_height
                        .saturating_sub(self.latest().height())
                        < 12
                }))
        {
            return Err("EXPIRY_MARGIN");
        }
        let proofs=order_hashes.iter().map(|h| {
            let order=&self.orders[h];let (raw,sig)=self.evidence("ORDER",&order.live.owner,&format!("{}:{}",order.epoch,order.order_id)).ok_or("ORDER_NOT_FOUND")?;
            Ok(json!({"order":Codec::default().decode("OrderV1",raw)?,"signature":STANDARD.encode(sig)}))
        }).collect::<Result<Vec<_>>>()?;
        let fills=ids.iter().map(|id| {let f=&self.outbox[id];json!({"fill_id":id,"maker_order_ref":f["maker_order_hash"],"taker_order_ref":f["taker_order_hash"],"buyer_order_ref":f["buyer_order_hash"],"seller_order_ref":f["seller_order_hash"],"quantity_lots":f["quantity_lots"],"execution_price_ticks":f["execution_price_ticks"],"fee_policy_version":f["fee_policy_version"],"command_seq":f["command_seq"],"match_index":f["match_index"]})}).collect::<Vec<_>>();
        let context = self.latest().context();
        let (raw, batch) = wire::seal(
            json!({"protocol_version":"2","chain_id":context["chain_id"],"genesis_hash":context["genesis_hash"],"exchange_module_id":"x/exchange","market_id":context["market_id"],"market_config_version":context["market_config_version"],"operator_epoch":epoch.to_string(),"batch_seq":self.latest().last_seq().checked_add(1).ok_or("INTEGER_OVERFLOW")?.to_string(),"previous_batch_hash":self.latest().value()["last_batch_hash"],"batch_id":schema::ZERO,"new_signed_orders":proofs,"fills":fills}),
        )?;
        let mut n = self.next()?;
        for id in &ids {
            let f = n.outbox.get_mut(id).unwrap();
            f["batch"] = batch.clone();
            f["export_state"] = json!("SEALED_S3");
            bump(f)?;
        }
        n.batch_wires
            .insert(batch["batch_id"].as_str().unwrap().into(), raw);
        n.batches.push(json!({"context":context,"batch":batch,"state":"SEALED","revision":"1","observed_height":self.latest().height().to_string(),"reason":"","seal_purpose":purpose,"attempt_hashes":[],"receipt":null}));
        Ok(n)
    }
    /// Must be called and durably committed before a worker broadcasts. Even an
    /// unbroadcast PREPARED envelope is unresolved after process death.
    pub fn prepare_attempt(&self, a: Value) -> Result<Self> {
        schema::validate("Attempt", &a)?;
        if let Some(old) = self.attempts.iter().find(|v| v["tx_hash"] == a["tx_hash"]) {
            return if old == &a {
                Ok(self.clone())
            } else {
                Err("ATTEMPT_CONFLICT")
            };
        }
        let ix = self.active()?;
        let b = &self.batches[ix];
        let batch = &b["batch"];
        if a["context"] != *self.latest().context()
            || a["batch"] != *batch
            || a["state"] != "PREPARED"
            || a["broadcast_count"] != "0"
            || !a["confirmed_tx"].is_null()
            || !a["absence_proof"].is_null()
            || a["operator"] != self.latest().value()["operator"]
            || a["operator_epoch"] != self.latest().value()["operator_epoch"]
        {
            return Err("ATTEMPT_CONFLICT");
        }
        let kind = a["kind"].as_str().unwrap();
        let prior: Vec<_> = self
            .attempts
            .iter()
            .filter(|a| a["batch"] == *batch && a["kind"] == kind)
            .collect();
        if self.attempts.iter().any(|a| {
            a["batch"] == *batch
                && ["PREPARED", "SUBMISSION_UNKNOWN"].contains(&a["state"].as_str().unwrap())
        }) {
            return Err("ATTEMPT_UNRESOLVED");
        }
        let (max, gas, fee) = if kind == "SETTLE" {
            (3, "10000000", "20000")
        } else {
            (2, "3000000", "6000")
        };
        if prior.len() >= max || num(&a["attempt_no"])? != prior.len() as u64 + 1 {
            return Err("RETRY_BUDGET_EXHAUSTED");
        }
        if a["gas_limit"] != gas
            || a["fee_atoms"] != fee
            || self.latest().height().checked_add(1) != Some(num(&a["first_possible_height"])?)
            || self.latest().height().checked_add(8) != Some(num(&a["timeout_height"])?)
        {
            return Err("ATTEMPT_POLICY");
        }
        if kind == "SETTLE"
            && !["SEALED", "SUBMISSION_UNKNOWN"].contains(&b["state"].as_str().unwrap())
        {
            return Err("BATCH_STATE");
        }
        if kind == "SETTLE" && prior.is_empty() && b["seal_purpose"] == "NORMAL" {
            for id in batch["fill_ids"].as_array().unwrap() {
                for k in ["buyer_order_hash", "seller_order_hash"] {
                    let h = self.outbox[id.as_str().unwrap()][k].as_str().unwrap();
                    if self.orders[h]
                        .live
                        .expiry_height
                        .saturating_sub(self.latest().height())
                        < 12
                    {
                        return Err("EXPIRY_MARGIN");
                    }
                }
            }
        }
        if kind == "CLOSE" && !["REJECTED_FINAL", "CLOSING"].contains(&b["state"].as_str().unwrap())
        {
            return Err("BATCH_STATE");
        }
        let audit = wire::attempt_envelope(
            &a,
            self.batch_wire(batch["batch_id"].as_str().unwrap())
                .unwrap(),
        )?;
        if kind == "CLOSE" {
            let e = self
                .failure_evidence
                .get(batch["batch_id"].as_str().unwrap())
                .ok_or("FAILURE_EVIDENCE_REQUIRED")?;
            if audit
                != Some((
                    e["failed_tx_hash"].as_str().unwrap().into(),
                    schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", e)?,
                ))
            {
                return Err("FAILURE_EVIDENCE_REQUIRED");
            }
        }
        let mut n = self.next()?;
        n.attempts.push(a.clone());
        n.batches[ix]["attempt_hashes"]
            .as_array_mut()
            .unwrap()
            .push(a["tx_hash"].clone());
        n.batches[ix]["state"] = json!(if kind == "SETTLE" {
            "SUBMISSION_UNKNOWN"
        } else {
            "CLOSING"
        });
        bump(&mut n.batches[ix])?;
        let ids: Vec<_> = batch["fill_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().into())
            .collect();
        n.graph.submission_unknown(&ids)?;
        for id in ids {
            let f = n.outbox.get_mut(&id).unwrap();
            if f["state"] != "SUBMISSION_UNKNOWN" {
                f["state"] = json!("SUBMISSION_UNKNOWN");
                bump(f)?;
            }
        }
        Ok(n)
    }
    /// Full attempt replacement is checked against the immutable envelope and
    /// monotonic counters. Timeout/NOT_FOUND are represented only as UNKNOWN.
    pub fn resolve_attempt(&self, a: Value) -> Result<Self> {
        schema::validate("Attempt", &a)?;
        let ix = self
            .attempts
            .iter()
            .position(|v| v["tx_hash"] == a["tx_hash"])
            .ok_or("ATTEMPT_NOT_FOUND")?;
        let old = &self.attempts[ix];
        if old == &a {
            return Ok(self.clone());
        }
        for (k, v) in old.as_object().unwrap() {
            if !["state", "broadcast_count", "confirmed_tx", "absence_proof"].contains(&k.as_str())
                && a[k] != *v
            {
                return Err("ATTEMPT_CONFLICT");
            }
        }
        let from = old["state"].as_str().unwrap();
        let to = a["state"].as_str().unwrap();
        if !["PREPARED", "SUBMISSION_UNKNOWN"].contains(&from)
            || num(&a["broadcast_count"])? < num(&old["broadcast_count"])?
            || num(&a["broadcast_count"])? > 3
        {
            return Err("ATTEMPT_CONFLICT");
        }
        match to {
            "PREPARED" | "SUBMISSION_UNKNOWN" => {
                if !a["confirmed_tx"].is_null() || !a["absence_proof"].is_null() {
                    return Err("ATTEMPT_CONFLICT");
                }
            }
            "INCLUDED_SUCCESS" | "INCLUDED_FAILURE" => {
                if !a["absence_proof"].is_null() {
                    return Err("ATTEMPT_CONFLICT");
                }
                let t = &a["confirmed_tx"];
                proof::confirmed(t, self.latest().context(), &self.history_refs())?;
                if t["tx_hash"] != a["tx_hash"]
                    || t["raw_tx"] != a["raw_tx"]
                    || num(&t["height"])? < num(&a["first_possible_height"])?
                    || num(&t["height"])? > num(&a["timeout_height"])?
                    || (t["abci_code"] == "0") != (to == "INCLUDED_SUCCESS")
                {
                    return Err("ATTEMPT_CONFLICT");
                }
            }
            "EXPIRED_ABSENT_PROVEN" => {
                if !a["confirmed_tx"].is_null() {
                    return Err("ATTEMPT_CONFLICT");
                }
                proof::absence(&a, self.latest(), &self.history_refs())?;
            }
            _ => return Err("ATTEMPT_CONFLICT"),
        }
        let mut n = self.next()?;
        n.attempts[ix] = a;
        Ok(n)
    }
    pub fn reject_final(&self, e: Value) -> Result<Self> {
        schema::validate("ResolutionEvidence", &e)?;
        let ix = self.active()?;
        let b = &self.batches[ix];
        let id = b["batch"]["batch_id"].as_str().unwrap();
        if let Some(old) = self.failure_evidence.get(id) {
            return if old == &e {
                Ok(self.clone())
            } else {
                Err("FAILURE_EVIDENCE_CONFLICT")
            };
        }
        let attempts: Vec<_> = self
            .attempts
            .iter()
            .filter(|a| a["batch"] == b["batch"] && a["kind"] == "SETTLE")
            .cloned()
            .collect();
        let lookup = &e["batch_lookup"];
        if e["context"] != *self.latest().context()
            || e["batch"] != b["batch"]
            || e["observed_snapshot"] != *self.latest().value()
            || e["settle_attempts"] != json!(attempts)
            || attempts.is_empty()
            || lookup["context"] != e["context"]
            || lookup["snapshot_id"] != self.latest().id()
            || num(&lookup["observed_height"])? != self.latest().height()
            || lookup["requested_seq"] != b["batch"]["batch_seq"]
            || lookup["last_seq"] != self.latest().value()["last_batch_seq"]
            || lookup["last_hash"] != self.latest().value()["last_batch_hash"]
            || lookup["status"] != "NOT_FOUND_AT_HEIGHT"
            || !lookup["receipt"].is_null()
            || num(&b["batch"]["batch_seq"])? <= self.latest().last_seq()
        {
            return Err("FAILURE_EVIDENCE_CONFLICT");
        }
        if attempts.iter().any(|a| {
            !["INCLUDED_FAILURE", "EXPIRED_ABSENT_PROVEN"].contains(&a["state"].as_str().unwrap())
        }) {
            return Err("ATTEMPT_UNRESOLVED");
        }
        let failed = attempts
            .iter()
            .find(|a| a["tx_hash"] == e["failed_tx_hash"] && a["state"] == "INCLUDED_FAILURE")
            .ok_or("FAILURE_EVIDENCE_REQUIRED")?;
        let codes: Value =
            serde_json::from_str(include_str!("../../../protocol/s3/errors.json")).unwrap();
        let reason = e["rejection_code"].as_str().unwrap();
        let expected = [
            "EPOCH_MISMATCH",
            "ORDER_REVOKED",
            "EXPIRED",
            "INSUFFICIENT_CONFIRMED_BALANCE",
            "OPERATOR_EPOCH_MISMATCH",
        ]
        .contains(&reason)
            && failed["confirmed_tx"]["codespace"] == "exchange_s3"
            && failed["confirmed_tx"]["abci_code"] == codes["abci_codes"][reason]
            && (reason != "OPERATOR_EPOCH_MISMATCH"
                || num(&b["batch"]["operator_epoch"])? < self.latest().operator_epoch());
        let mut n = self.next()?;
        n.failure_evidence.insert(id.into(), e.clone());
        n.batches[ix]["state"] = json!(if expected {
            "REJECTED_FINAL"
        } else {
            "RECOVERY_REQUIRED"
        });
        n.batches[ix]["reason"] = json!(if expected {
            reason
        } else {
            "UNEXPECTED_FINAL_REJECTION"
        });
        bump(&mut n.batches[ix])?;
        Ok(n)
    }
    /// Validate a terminal receipt without releasing any hold. This permits a
    /// later invalid queued prefix to be closed while a successful earlier slot
    /// waits for one joint C/R/D/P reconciliation. Only unresolved slots count
    /// against the one-inflight limit. The frozen state exposes apply pending.
    pub fn record_receipt(&self, r: Value) -> Result<Self> {
        schema::validate("ResolutionReceipt", &r)?;
        if let Some(old) = self
            .resolutions
            .iter()
            .find(|x| x["batch"]["batch_id"] == r["batch"]["batch_id"])
        {
            return if old == &r {
                Ok(self.clone())
            } else {
                Err("RECEIPT_INCONSISTENCY")
            };
        }
        let ix = self
            .batches
            .iter()
            .position(|b| b["batch"] == r["batch"])
            .ok_or("BATCH_NOT_FOUND")?;
        let b = &self.batches[ix];
        proof::receipt(&r, &b["batch"], self.latest(), &self.history_refs())?;
        let tx = &r["terminal_tx"];
        let attempt = self
            .attempts
            .iter()
            .find(|a| a["tx_hash"] == tx["tx_hash"] && a["batch"] == b["batch"])
            .ok_or("ATTEMPT_NOT_FOUND")?;
        if attempt["raw_tx"] != tx["raw_tx"]
            || num(&tx["height"])? < num(&attempt["first_possible_height"])?
            || num(&tx["height"])? > num(&attempt["timeout_height"])?
        {
            return Err("RECEIPT_INCONSISTENCY");
        }
        if r["disposition"] == "COMMITTED" {
            if attempt["kind"] != "SETTLE" || b["state"] == "CORRECTED" {
                return Err("COMMITTED_IMMUTABLE");
            }
        } else {
            let id = b["batch"]["batch_id"].as_str().unwrap();
            let e = self
                .failure_evidence
                .get(id)
                .ok_or("FAILURE_EVIDENCE_REQUIRED")?;
            if attempt["kind"] != "CLOSE"
                || b["state"] != "CLOSING"
                || r["failed_tx_hash"] != e["failed_tx_hash"]
                || r["resolution_evidence_hash"]
                    != schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", e)?
            {
                return Err("FAILURE_EVIDENCE_REQUIRED");
            }
        }
        let h = num(&tx["height"])?;
        let at = self
            .history
            .iter()
            .find(|s| s.height() == h)
            .ok_or("PROOF_HISTORY_GAP")?;
        let seq = num(&b["batch"]["batch_seq"])?;
        if !at.value()["terminal_batch_seqs"]
            .as_array()
            .unwrap()
            .contains(&b["batch"]["batch_seq"])
            || at.last_seq() < seq
            || (at.last_seq() == seq && at.value()["last_batch_hash"] != b["batch"]["batch_hash"])
        {
            return Err("RECEIPT_INCONSISTENCY");
        }
        let mut n = self.next()?;
        n.resolutions.push(r.clone());
        n.batches[ix]["receipt"] = json!({"context":r["context"],"batch":r["batch"],"disposition":r["disposition"],"terminal_height":tx["height"],"terminal_tx_hash":tx["tx_hash"],"batch_receipt_v2":r["batch_receipt_v2"]});
        n.batches[ix]["reason"] = json!("ENGINE_APPLY_PENDING");
        bump(&mut n.batches[ix])?;
        Ok(n)
    }
    /// Apply all newly resolved slots, confirmed C and accumulated owner events
    /// atomically. All intermediate candidates stay private. There is no P->C
    /// credit: C is taken exclusively from the manifest-validated chain snapshot.
    pub fn apply(&self) -> Result<Self> {
        if self.observations.is_empty() {
            return Ok(self.clone());
        }
        let target = self.latest();
        let mut n = self.next()?;
        let mut committed = vec![];
        let mut voids = vec![];
        let mut last = self.snapshot.last_seq();
        let mut previous = self.snapshot.value()["last_batch_hash"].clone();
        for b in &self.batches {
            let seq = num(&b["batch"]["batch_seq"])?;
            if seq <= last {
                continue;
            }
            if seq > target.last_seq() {
                break;
            }
            if last.checked_add(1) != Some(seq) || b["batch"]["previous_batch_hash"] != previous {
                return Err("RECEIPT_INCONSISTENCY");
            }
            let r = self
                .resolutions
                .iter()
                .find(|r| r["batch"] == b["batch"])
                .ok_or("UNSETTLED_HOLD")?;
            let ids: Vec<String> = b["batch"]["fill_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().into())
                .collect();
            if r["disposition"] == "COMMITTED" {
                committed.extend(ids);
            } else {
                voids.push((b.clone(), r.clone(), ids));
            }
            last = seq;
            previous = b["batch"]["batch_hash"].clone();
        }
        if last != target.last_seq() || previous != target.value()["last_batch_hash"] {
            return Err("RECEIPT_INCONSISTENCY");
        }
        n.graph.committed(&committed)?;
        let mut roots = vec![];
        for (_, _, ids) in &voids {
            roots.extend(ids.iter().cloned());
        }
        for id in &self.fill_order {
            let f = &self.outbox[id];
            if n.graph.node(id).unwrap().state.pending()
                && (["buyer_order_hash", "seller_order_hash"]
                    .iter()
                    .any(|k| self.invalid_order(f[*k].as_str().unwrap()))
                    || num(&f["origin_operator_epoch"])? != target.operator_epoch())
            {
                roots.push(id.clone());
            }
        }
        roots.sort();
        roots.dedup();
        if !roots.is_empty() && voids.is_empty() {
            return Err("UNSETTLED_HOLD");
        }
        let closure = n.graph.closure(&roots)?;
        // An unresolved sealed candidate cannot be changed by another closure.
        for id in &closure.corrected {
            let f = &self.outbox[id];
            if !f["batch"].is_null() && !voids.iter().any(|(b, _, _)| b["batch"] == f["batch"]) {
                return Err("ATTEMPT_UNRESOLVED");
            }
        }
        let mut affected = closure.affected_orders.clone();
        let mut cancel = vec![];
        let mut ordered: Vec<_> = self.orders.values().collect();
        ordered.sort_by_key(|o| o.live.admission_seq);
        for o in &ordered {
            let domain = DebitDomain {
                owner: o.live.owner.clone(),
                epoch: o.epoch,
                asset: if o.live.side == Side::Buy {
                    DebitAsset::Quote
                } else {
                    DebitAsset::Base
                },
            };
            if o.live.remaining > 0
                && (closure.affected_debits.contains(&domain)
                    || self.invalid_order(&o.live.hash)
                    || o.live.expiry_height <= target.height()
                    || self.snapshot.operator_epoch() != target.operator_epoch())
            {
                cancel.push(o.live.hash.clone());
                affected.insert(o.live.hash.clone());
            }
        }
        n.graph.corrected(&roots)?;
        let confirmed = target
            .accounts()
            .iter()
            .map(|a| (a.owner.clone(), a.confirmed))
            .collect();
        n.ledger
            .reconcile(&confirmed, &committed, &closure.corrected, &cancel)?;
        for id in &cancel {
            let o = n.orders.get_mut(id).unwrap();
            o.live.remaining = 0;
            o.status = if self.invalid_order(id) {
                "REVOKED_ONCHAIN"
            } else if o.live.expiry_height <= target.height() {
                "EXPIRED"
            } else {
                "CORRECTED"
            }
            .into();
        }
        for (ids, state) in [(&committed, "COMMITTED"), (&closure.corrected, "CORRECTED")] {
            for id in ids {
                let f = n.outbox.get_mut(id).unwrap();
                if f["state"] != state {
                    f["state"] = json!(state);
                    f["export_state"] = json!("TERMINAL_S3");
                    f["reason"] = json!(if state == "COMMITTED" {
                        ""
                    } else {
                        "FINAL_REJECTION"
                    });
                    bump(f)?;
                }
            }
        }
        for o in n.orders.values_mut() {
            let q = n.ledger.quantities(&o.live.hash)?;
            if q.corrected > 0 {
                o.status = "CORRECTED".into();
            } else if q.lifetime_matched > 0
                && q.pending == 0
                && o.live.remaining == 0
                && o.status == "FILLED_PENDING"
            {
                o.status = "FILLED_COMMITTED".into();
            }
        }
        let affected_order_hashes: Vec<_> = ordered
            .iter()
            .filter(|o| affected.contains(&o.live.hash))
            .map(|o| o.live.hash.clone())
            .collect();
        // One correction per commit covers the minimal union closure; every
        // contributing VOID receipt is retained in resolution_receipts/applied.
        // The latest VOID identifies this joint correction, roots bind all inputs.
        if let Some((b, r, _)) = voids.last() {
            let correction_id = schema::hash(
                "NUS/S3/CORRECTION/V1",
                &json!({"context":target.context(),"void_batch":b["batch"],"snapshot_id":target.id(),"root_fill_ids":roots}),
            )?;
            let c = json!({"context":target.context(),"correction_id":correction_id,"void_batch":b["batch"],"resolution_receipt":r,"chain_snapshot_id":target.id(),"chain_height":target.height().to_string(),"root_fill_ids":roots,"corrected_fill_ids":closure.corrected,"affected_order_hashes":affected_order_hashes,"cancelled_order_hashes":cancel,"surviving_fill_ids":closure.surviving_pending,"before_state_hash":self.full_hash()?,"command_seq":n.seq.to_string(),"revision":"1"});
            schema::validate("CorrectionRecord", &c)?;
            n.corrections.push(c);
        }
        for b in &mut n.batches {
            let seq = num(&b["batch"]["batch_seq"])?;
            if seq > self.snapshot.last_seq() && seq <= target.last_seq() {
                let r = self
                    .resolutions
                    .iter()
                    .find(|r| r["batch"] == b["batch"])
                    .unwrap();
                b["state"] = json!(if r["disposition"] == "COMMITTED" {
                    "COMMITTED"
                } else {
                    "CORRECTED"
                });
                b["reason"] = json!("");
                b["observed_height"] = json!(target.height().to_string());
                bump(b)?;
                n.applied.push(json!({"batch_id":b["batch"]["batch_id"],"receipt_hash":schema::hash("NUS/S3/VIEW/V1",r)?,"revision":b["revision"],"command_seq":n.seq.to_string(),"snapshot_id":target.id()}));
            }
        }
        n.snapshot = target.clone();
        n.observations.clear();
        n.revise_orders(self)?;
        n.full_state()?;
        Ok(n)
    }
    pub fn evidence_objects(&self) -> Result<Vec<(Vec<u8>, String)>> {
        let mut objects = Vec::new();
        for a in &self.attempts {
            objects.push((
                canonical(a).map_err(|_| "S3_CANONICAL")?,
                "application/json".into(),
            ));
        }
        if !self.observations.is_empty() {
            objects.push((
                canonical(&json!(
                    self.observations
                        .iter()
                        .map(|s| s.value())
                        .collect::<Vec<_>>()
                ))
                .map_err(|_| "S3_CANONICAL")?,
                "application/json".into(),
            ));
        }
        Ok(objects)
    }
}

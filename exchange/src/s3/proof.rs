//! Verification of raw trusted-local RPC inclusion and absence observations.
//! This is not cryptographic consensus verification. JSON wrappers never replace
//! raw block/index/hash/results checks, and NOT_FOUND is never failure proof.
use super::{
    evidence::{Objects, RPC, TX},
    journal::sha256,
    schema::{self, bytes, num},
    snapshot::Snapshot,
};
use crate::{
    Result,
    codec::{self, Codec},
};
use serde_json::Value;
fn rpc_num(v: &Value) -> Result<u64> {
    if let Some(n) = v.as_u64() {
        Ok(n)
    } else {
        num(v)
    }
}
pub fn block(v: &Value, context: &Value, objects: &Objects) -> Result<(Value, Value)> {
    let rb = objects.resolve(&v["raw_block_response_ref"], RPC)?;
    let rr = objects.resolve(&v["raw_results_response_ref"], RPC)?;
    let b = codec::unique_json(rb)?;
    let r = codec::unique_json(rr)?;
    let b = b.get("result").ok_or("RAW_BLOCK")?.clone();
    let r = r.get("result").ok_or("RAW_RESULTS")?.clone();
    let bh = b["block_id"]["hash"]
        .as_str()
        .ok_or("RAW_BLOCK")?
        .to_lowercase();
    if bh != v["block_hash"]
        || b["block"]["header"]["height"] != v["height"]
        || r["height"] != v["height"]
        || b["block"]["header"]["chain_id"] != context["chain_id"]
    {
        return Err("RAW_BLOCK_BINDING");
    }
    let ntx = if b["block"]["data"]["txs"].is_null() {
        0
    } else {
        b["block"]["data"]["txs"]
            .as_array()
            .ok_or("RAW_BLOCK")?
            .len()
    };
    let nr = if r["txs_results"].is_null() {
        0
    } else {
        r["txs_results"].as_array().ok_or("RAW_RESULTS")?.len()
    };
    if ntx != nr {
        return Err("RAW_RESULTS");
    }
    Ok((b, r))
}
pub fn confirmed(
    v: &Value,
    context: &Value,
    history: &[&Snapshot],
    objects: &Objects,
) -> Result<()> {
    schema::validate("ConfirmedTx", v)?;
    let (b, r) = block(v, context, objects)?;
    let h = num(&v["height"])?;
    let snapshot = history
        .iter()
        .find(|s| s.height() == h)
        .ok_or("PROOF_HISTORY_GAP")?;
    if snapshot.value()["block_hash"] != v["block_hash"] {
        return Err("PROOF_HISTORY_CONFLICT");
    }
    let raw = objects.resolve(&v["raw_tx_ref"], TX)?;
    let ix = num(&v["tx_index"])? as usize;
    let txs = b["block"]["data"]["txs"].as_array().ok_or("RAW_BLOCK")?;
    let entry = r["txs_results"]
        .as_array()
        .and_then(|x| x.get(ix))
        .ok_or("RAW_RESULTS")?;
    if txs.get(ix).map(bytes).transpose()?.as_deref() != Some(raw)
        || sha256(raw) != v["tx_hash"]
        || rpc_num(&entry["code"])? != num(&v["abci_code"])?
        || entry["codespace"] != v["codespace"]
        || rpc_num(&entry["gas_wanted"])? != num(&v["gas_wanted"])?
        || rpc_num(&entry["gas_used"])? != num(&v["gas_used"])?
    {
        return Err("CONFIRMED_TX_MISMATCH");
    }
    Ok(())
}
pub fn absence(
    a: &Value,
    snapshot: &Snapshot,
    history: &[&Snapshot],
    objects: &Objects,
) -> Result<()> {
    let p = &a["absence_proof"];
    schema::validate("AbsenceProof", p)?;
    let first = num(&a["first_possible_height"])?;
    let timeout = num(&a["timeout_height"])?;
    if timeout.checked_sub(first).and_then(|n| n.checked_add(1)) != Some(8)
        || p["tx_hash"] != a["tx_hash"]
        || p["first_possible_height"] != a["first_possible_height"]
        || p["timeout_height"] != a["timeout_height"]
        || num(&p["observed_height"])? != snapshot.height()
        || snapshot.height() <= timeout
        || p["observation_snapshot_id"] != snapshot.id()
        || p["last_batch_seq"] != snapshot.value()["last_batch_seq"]
        || p["last_batch_hash"] != snapshot.value()["last_batch_hash"]
        || p["receipt_absent"] != true
        || num(&p["account_sequence"])? < num(&a["account_sequence"])?
    {
        return Err("ABSENCE_PROOF");
    }
    let blocks = p["blocks"].as_array().unwrap();
    if blocks.len() != 8 {
        return Err("ABSENCE_GAP");
    }
    let mut previous: Option<String> = None;
    for (i, v) in blocks.iter().enumerate() {
        let h = first.checked_add(i as u64).ok_or("INTEGER_OVERFLOW")?;
        let s = history
            .iter()
            .find(|s| s.height() == h)
            .ok_or("PROOF_HISTORY_GAP")?;
        if num(&v["height"])? != h || s.value()["block_hash"] != v["block_hash"] {
            return Err("ABSENCE_GAP");
        }
        let (b, _) = block(v, snapshot.context(), objects)?;
        if previous.as_ref().is_some_and(|p| {
            b["block"]["header"]["last_block_id"]["hash"]
                .as_str()
                .is_none_or(|h| h.to_lowercase() != *p)
        }) {
            return Err("ABSENCE_HISTORY");
        }
        previous = Some(v["block_hash"].as_str().unwrap().into());
        if let Some(txs) = b["block"]["data"]["txs"].as_array() {
            for tx in txs {
                if sha256(&bytes(tx)?) == a["tx_hash"] {
                    return Err("ABSENCE_TX_FOUND");
                }
            }
        }
    }
    Ok(())
}
pub fn receipt(
    r: &Value,
    batch: &Value,
    snapshot: &Snapshot,
    history: &[&Snapshot],
    objects: &Objects,
) -> Result<()> {
    schema::validate("ResolutionReceipt", r)?;
    if r["context"] != *snapshot.context()
        || r["batch"] != *batch
        || num(&r["terminal_tx"]["height"])? > snapshot.height()
        || r["terminal_tx"]["abci_code"] != "0"
    {
        return Err("RECEIPT_INCONSISTENCY");
    }
    confirmed(&r["terminal_tx"], snapshot.context(), history, objects)?;
    if r["disposition"] == "COMMITTED" {
        if !r["failed_tx_hash"].is_null()
            || !r["resolution_evidence_hash"].is_null()
            || !r["resolution_evidence_ref"].is_null()
        {
            return Err("RECEIPT_INCONSISTENCY");
        }
        let v = Codec::default().decode("BatchReceiptV1", &bytes(&r["batch_receipt_v2"])?)?;
        if v["protocol_version"] != "2"
            || v["committed_height"] != r["terminal_tx"]["height"]
            || v["tx_hash"] != r["terminal_tx"]["tx_hash"]
        {
            return Err("RECEIPT_INCONSISTENCY");
        }
        for k in ["chain_id", "genesis_hash", "market_id"] {
            if v[k] != snapshot.context()[k] {
                return Err("RECEIPT_INCONSISTENCY");
            }
        }
        for k in ["batch_seq", "batch_id", "batch_hash"] {
            if v[k] != batch[k] {
                return Err("RECEIPT_INCONSISTENCY");
            }
        }
    } else if !r["batch_receipt_v2"].is_null()
        || r["failed_tx_hash"].is_null()
        || r["resolution_evidence_hash"].is_null()
        || r["resolution_evidence_ref"].is_null()
    {
        return Err("RECEIPT_INCONSISTENCY");
    }
    Ok(())
}

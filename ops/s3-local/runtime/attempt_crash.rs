//! F04-only Attempt crash boundary.
//!
//! The selected C hook is deliberately fixed: TxRaw evidence and the Attempt
//! WAL frame have been fsynced, while the commit marker has not been opened.
//! A reached hook exits the isolated child with 86.  The remaining tail is
//! UNKNOWN and must never authorize a replacement envelope or automatic repair.
#[path = "storage_crash.rs"]
mod storage_crash;

use nus_exchange_contract::s3::{
    dev_local::{Engine, Error, Result},
    evidence::{self, TX},
    journal::{canonical, sha256},
    schema,
    settlement_local::Worker,
    snapshot::Observation,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

pub use storage_crash::Report;

pub const POINT: &str = "after_wal_sync";
pub const EXIT: i32 = storage_crash::CRASH_EXIT;

/// Persist an exact F04 command reservation and execute one approved C Attempt.
///
/// This is not reachable from the ordinary scheduler.  The caller must own the
/// Engine exclusively and drop it after any return.  The function never signs,
/// broadcasts, retries, repairs or truncates the store.
pub fn run_recorded(
    engine: Arc<Engine>,
    attempt: Value,
    tx: Vec<u8>,
    observation: &Observation,
    now_ms: u64,
    evidence_root: &Path,
) -> Result<(Result<()>, Report)> {
    let tx_hash = sha256(&tx);
    if attempt["kind"] != "SETTLE"
        || attempt["state"] != "PREPARED"
        || attempt["broadcast_count"] != "0"
        || attempt["tx_hash"] != tx_hash
        || attempt["raw_tx_ref"] != evidence::reference(&tx, TX)?
        || observation.catching_up
    {
        return Err(Error::Invalid("F04_ATTEMPT_INPUT"));
    }
    let command = serde_json::to_vec(&json!({
        "schema": "s3-local-f04-attempt-crash/1",
        "fault_id": "F04",
        "command": "Attempt",
        "point": POINT,
        "occurrence": "1",
        "effect": "IMMEDIATE_EXIT",
        "exit_code": EXIT,
        "expected": "UNKNOWN_TAIL_NO_NEW_ENVELOPE",
        "attempt_sha256": sha256(&canonical(&attempt)?),
        "tx_sha256": tx_hash,
        "batch_id": attempt["batch"]["batch_id"],
        "attempt_no": schema::num(&attempt["attempt_no"])?.to_string(),
        "observation": {
            "snapshot_id": observation.snapshot_id,
            "cursor_height": observation.cursor_height.to_string(),
            "received_at": observation.received_at.to_string(),
            "query_latency_ms": observation.query_latency_ms.to_string(),
            "catching_up": observation.catching_up,
        },
        "now_ms": now_ms.to_string(),
        "enable_dev_local_demo": true,
        "allow_unproven_host_space": true,
    }))
    .map_err(|_| Error::Recovery("ENCODING"))?;
    let crash = storage_crash::StorageCrash::new(POINT, 1, true, true)?;
    let worker = Worker::new(engine.clone());
    storage_crash::run_recorded_command(&engine, crash, evidence_root, &command, || {
        worker.prepare(attempt, &[(tx, TX.into())], observation, now_ms)
    })
}

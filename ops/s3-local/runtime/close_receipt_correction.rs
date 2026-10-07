//! Fault-build-only F13 boundary: durable CLOSE/VOID receipt before the
//! correction plan. This module never calls Apply or creates a replacement
//! batch; C remains the sole owner of closure and correction semantics.
#![cfg(feature = "fault-injection")]

use base64::Engine as _;
use nus_exchange_contract::s3::{
    dev_local::{Engine, Error, Result},
    journal::canonical,
};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    pub batch_id: String,
    pub receipt_sha256: String,
    pub before_batch_count: usize,
    boundary: Vec<u8>,
}

impl Reached {
    pub fn boundary_sha256(&self) -> String {
        hex::encode(Sha256::digest(&self.boundary))
    }

    fn record<T>(
        &self,
        directory: &std::path::Path,
        action: impl FnOnce(&Reached) -> Result<T>,
    ) -> Result<T> {
        use std::{
            fs::{File, OpenOptions},
            io::Write,
            os::{
                fd::{AsRawFd, FromRawFd},
                unix::fs::{MetadataExt, OpenOptionsExt},
            },
        };
        if !directory.is_absolute() || directory.canonicalize()? != directory {
            return Err(Error::Invalid("F13_REPORT_ROOT"));
        }
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(directory)?;
        let rm = root.metadata()?;
        if rm.uid() != unsafe { libc::geteuid() } || rm.mode() & 0o7777 != 0o700 {
            return Err(Error::Invalid("F13_REPORT_ROOT"));
        }
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                c"close-receipt-correction.jsonl".as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let fm = file.metadata()?;
        let mut append = |phase: &str| -> Result<()> {
            let current_root = std::fs::symlink_metadata(directory)?;
            let current_file =
                std::fs::symlink_metadata(directory.join("close-receipt-correction.jsonl"))?;
            if (current_root.dev(), current_root.ino(), current_root.mode(), current_root.uid())
                != (rm.dev(), rm.ino(), rm.mode(), rm.uid())
                || (current_file.dev(), current_file.ino(), current_file.mode(), current_file.uid(), current_file.nlink())
                    != (fm.dev(), fm.ino(), fm.mode(), fm.uid(), 1)
                || fm.mode() & 0o7777 != 0o600
            {
                return Err(Error::Invalid("F13_REPORT_CHANGED"));
            }
            let row = json!({
                "schema":"s3-local-close-receipt-correction/1", "phase":phase,
                "boundary_base64":base64::engine::general_purpose::STANDARD.encode(&self.boundary),
                "boundary_sha256":self.boundary_sha256(),
                "receipt_sha256":self.receipt_sha256, "batch_id":self.batch_id,
                "before_batch_count":self.before_batch_count.to_string(),
                "correction_plan_visible":false, "replacement_seq_created":false,
                "apply_called":false, "crash_verified":false,
                "durable_ack":false, "DEV":"NOT_RUN"
            });
            file.write_all(&serde_json::to_vec(&row).map_err(|_| Error::Invalid("F13_REPORT_JSON"))?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            root.sync_all()?;
            Ok(())
        };
        append("reserved")?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(self)));
        let phase = match &outcome {
            Ok(Ok(_)) => "boundary_returned",
            Ok(Err(_)) => "boundary_error",
            Err(_) => "panic",
        };
        let persisted = append(phase);
        match outcome {
            Err(payload) => std::panic::resume_unwind(payload),
            Ok(result) => {
                persisted?;
                result
            }
        }
    }
}

/// Persist exactly one approved VOID receipt and enter F13 only while the
/// original batch remains CLOSING and no correction or replacement batch is
/// visible. The caller consumes its lane and this function never retries.
pub fn run<T>(
    engine: &Engine,
    expected_batch: &str,
    directory: &std::path::Path,
    persist_void_receipt: impl FnOnce() -> Result<()>,
    boundary: impl FnOnce(&Reached) -> Result<T>,
) -> Result<T> {
    if expected_batch.len() != 64
        || !expected_batch.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Invalid("F13_BATCH"));
    }
    let before = engine.reader().get()?;
    let before_batches = before.state["batches"]
        .as_array()
        .ok_or(Error::Invalid("F13_BATCHES"))?;
    let old_receipts = before.state["resolution_receipts"]
        .as_array()
        .ok_or(Error::Invalid("F13_RECEIPTS"))?
        .len();
    let old_corrections = before.state["corrections"]
        .as_array()
        .ok_or(Error::Invalid("F13_CORRECTIONS"))?
        .len();
    let original = before_batches
        .iter()
        .find(|entry| entry["batch"]["batch_id"] == expected_batch)
        .ok_or(Error::Invalid("F13_BATCH"))?;
    if original["state"] != "CLOSING" || !original["receipt"].is_null() {
        return Err(Error::Invalid("F13_CLOSE_NOT_READY"));
    }
    persist_void_receipt()?;
    let after = engine.reader().get()?;
    let after_batches = after.state["batches"]
        .as_array()
        .ok_or(Error::Invalid("F13_BATCHES"))?;
    let receipts = after.state["resolution_receipts"]
        .as_array()
        .ok_or(Error::Invalid("F13_RECEIPTS"))?;
    let corrections = after.state["corrections"]
        .as_array()
        .ok_or(Error::Invalid("F13_CORRECTIONS"))?;
    if receipts.len() != old_receipts + 1 || after.commit.command_seq <= before.commit.command_seq {
        return Err(Error::Recovery("F13_RECEIPT_NOT_PERSISTED"));
    }
    if after_batches.len() != before_batches.len() || corrections.len() != old_corrections {
        return Err(Error::Recovery("F13_REPLACEMENT_OR_CORRECTION_VISIBLE"));
    }
    for key in ["accounts", "fills", "chain_snapshot"] {
        if before.state[key] != after.state[key] {
            return Err(Error::Recovery("F13_APPLY_ALREADY_VISIBLE"));
        }
    }
    let current = after_batches
        .iter()
        .find(|entry| entry["batch"]["batch_id"] == expected_batch)
        .ok_or(Error::Invalid("F13_BATCH"))?;
    let receipt = receipts.last().ok_or(Error::Invalid("F13_RECEIPTS"))?;
    if current["state"] != "CLOSING"
        || current["reason"] != "ENGINE_APPLY_PENDING"
        || current["receipt"]["disposition"] != "VOID"
        || receipt["disposition"] != "VOID"
        || receipt["batch"]["batch_id"] != expected_batch
    {
        return Err(Error::Invalid("F13_RECEIPT_IDENTITY"));
    }
    let receipt_bytes = canonical(receipt)?;
    let receipt_sha256 = hex::encode(Sha256::digest(&receipt_bytes));
    let boundary_bytes = canonical(&json!({
        "schema":"sre-close-receipt-correction-boundary/1", "boundary":"F13",
        "batch_id":expected_batch, "receipt_sha256":receipt_sha256,
        "before_batch_count":before_batches.len().to_string(),
        "after_batch_count":after_batches.len().to_string(),
        "before_correction_count":old_corrections.to_string(),
        "after_correction_count":corrections.len().to_string(),
        "before_commit":{"command_seq":before.commit.command_seq.to_string(),
            "record_hash":before.commit.record_hash,"end_offset":before.commit.end_offset.to_string()},
        "after_commit":{"command_seq":after.commit.command_seq.to_string(),
            "record_hash":after.commit.record_hash,"end_offset":after.commit.end_offset.to_string()},
        "batch_state":"CLOSING", "batch_reason":"ENGINE_APPLY_PENDING",
        "correction_plan_visible":false, "replacement_seq_created":false,
        "apply_called":false, "reusable_permit":false
    }))?;
    Reached {
        batch_id: expected_batch.into(),
        receipt_sha256,
        before_batch_count: before_batches.len(),
        boundary: boundary_bytes,
    }
    .record(directory, boundary)
}

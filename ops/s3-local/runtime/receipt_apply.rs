//! Fault-build-only F09 boundary: one durable Receipt command, before Apply.
//! This module neither synthesizes a receipt nor calls Apply. It records a
//! provenance boundary only after the approved C store publishes the receipt.
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
    pub disposition: String,
    pub receipt_sha256: String,
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
            return Err(Error::Invalid("F09_REPORT_ROOT"));
        }
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(directory)?;
        let rm = root.metadata()?;
        if rm.uid() != unsafe { libc::geteuid() } || rm.mode() & 0o7777 != 0o700 {
            return Err(Error::Invalid("F09_REPORT_ROOT"));
        }
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                c"receipt-apply.jsonl".as_ptr(),
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
            let current_file = std::fs::symlink_metadata(directory.join("receipt-apply.jsonl"))?;
            if (
                current_root.dev(),
                current_root.ino(),
                current_root.mode(),
                current_root.uid(),
            ) != (rm.dev(), rm.ino(), rm.mode(), rm.uid())
                || (
                    current_file.dev(),
                    current_file.ino(),
                    current_file.mode(),
                    current_file.uid(),
                    current_file.nlink(),
                ) != (fm.dev(), fm.ino(), fm.mode(), fm.uid(), 1)
                || fm.mode() & 0o7777 != 0o600
            {
                return Err(Error::Invalid("F09_REPORT_CHANGED"));
            }
            let row = json!({
                "schema":"s3-local-receipt-apply/1", "phase":phase,
                "boundary_base64":base64::engine::general_purpose::STANDARD.encode(&self.boundary),
                "boundary_sha256":self.boundary_sha256(),
                "receipt_sha256":self.receipt_sha256, "batch_id":self.batch_id,
                "disposition":self.disposition, "apply_called":false,
                "crash_verified":false, "durable_ack":false, "DEV":"NOT_RUN"
            });
            file.write_all(
                &serde_json::to_vec(&row).map_err(|_| Error::Invalid("F09_REPORT_JSON"))?,
            )?;
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

/// Execute exactly one caller-supplied Receipt persistence operation, verify
/// that no economic Apply fields changed, then enter the recorded F09 boundary.
/// The caller must consume its lane; this function never retries.
pub fn run<T>(
    engine: &Engine,
    expected_batch: &str,
    directory: &std::path::Path,
    persist_receipt: impl FnOnce() -> Result<()>,
    boundary: impl FnOnce(&Reached) -> Result<T>,
) -> Result<T> {
    if expected_batch.len() != 64
        || !expected_batch
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Invalid("F09_BATCH"));
    }
    let before = engine.reader().get()?;
    let batch_known = before.state["batches"]
        .as_array()
        .ok_or(Error::Invalid("F09_BATCHES"))?
        .iter()
        .any(|entry| entry["batch"]["batch_id"] == expected_batch);
    if !batch_known {
        return Err(Error::Invalid("F09_BATCH"));
    }
    let old_receipts = before.state["resolution_receipts"]
        .as_array()
        .ok_or(Error::Invalid("F09_RECEIPTS"))?
        .len();
    persist_receipt()?;
    let after = engine.reader().get()?;
    let receipts = after.state["resolution_receipts"]
        .as_array()
        .ok_or(Error::Invalid("F09_RECEIPTS"))?;
    if receipts.len() != old_receipts + 1 || after.commit.command_seq <= before.commit.command_seq {
        return Err(Error::Recovery("F09_RECEIPT_NOT_PERSISTED"));
    }
    for key in ["accounts", "fills", "chain_snapshot", "corrections"] {
        if before.state[key] != after.state[key] {
            return Err(Error::Recovery("F09_APPLY_ALREADY_VISIBLE"));
        }
    }
    let receipt = receipts.last().ok_or(Error::Invalid("F09_RECEIPTS"))?;
    let batch_id = receipt["batch"]["batch_id"].as_str().ok_or("F09_BATCH")?;
    let disposition = receipt["disposition"].as_str().ok_or("F09_DISPOSITION")?;
    if batch_id != expected_batch || !matches!(disposition, "COMMITTED" | "VOID") {
        return Err(Error::Invalid("F09_RECEIPT_IDENTITY"));
    }
    let receipt_bytes = canonical(receipt)?;
    let boundary_bytes = canonical(&json!({
        "schema":"sre-receipt-apply-boundary/1", "boundary":"F09",
        "batch_id":batch_id, "disposition":disposition,
        "receipt_sha256":hex::encode(Sha256::digest(&receipt_bytes)),
        "before_commit":{"command_seq":before.commit.command_seq.to_string(),
            "record_hash":before.commit.record_hash,"end_offset":before.commit.end_offset.to_string()},
        "after_commit":{"command_seq":after.commit.command_seq.to_string(),
            "record_hash":after.commit.record_hash,"end_offset":after.commit.end_offset.to_string()},
        "apply_called":false,"reusable_permit":false
    }))?;
    let reached = Reached {
        batch_id: batch_id.into(),
        disposition: disposition.into(),
        receipt_sha256: hex::encode(Sha256::digest(receipt_bytes)),
        boundary: boundary_bytes,
    };
    reached.record(directory, boundary)
}

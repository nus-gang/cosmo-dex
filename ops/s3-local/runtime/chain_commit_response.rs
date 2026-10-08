//! Fault-build-only F08 boundary: trusted successful inclusion before the
//! settlement RPC response is delivered.  It neither persists a Receipt nor
//! applies an asset effect; recovery must query the original attempt.
#![cfg(feature = "fault-injection")]

use base64::Engine as _;
use nus_exchange_contract::s3::{
    dev_local::{Engine, Error, Result},
    journal::canonical,
    schema,
};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    pub tx_hash: String,
    pub batch_id: String,
    pub height: u64,
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
            return Err(Error::Invalid("F08_REPORT_ROOT"));
        }
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(directory)?;
        let rm = root.metadata()?;
        if rm.uid() != unsafe { libc::geteuid() } || rm.mode() & 0o7777 != 0o700 {
            return Err(Error::Invalid("F08_REPORT_ROOT"));
        }
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                c"chain-commit-response.jsonl".as_ptr(),
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
                std::fs::symlink_metadata(directory.join("chain-commit-response.jsonl"))?;
            if (current_root.dev(), current_root.ino(), current_root.mode(), current_root.uid())
                != (rm.dev(), rm.ino(), rm.mode(), rm.uid())
                || (current_file.dev(), current_file.ino(), current_file.mode(), current_file.uid(), current_file.nlink())
                    != (fm.dev(), fm.ino(), fm.mode(), fm.uid(), 1)
                || fm.mode() & 0o7777 != 0o600
            {
                return Err(Error::Invalid("F08_REPORT_CHANGED"));
            }
            let row = json!({
                "schema":"s3-local-chain-commit-response/1", "phase":phase,
                "boundary_base64":base64::engine::general_purpose::STANDARD.encode(&self.boundary),
                "boundary_sha256":self.boundary_sha256(),
                "tx_hash":self.tx_hash, "batch_id":self.batch_id,
                "response_delivered":false, "receipt_queried":false,
                "asset_effect_verified":false, "crash_verified":false,
                "durable_ack":false, "DEV":"NOT_RUN"
            });
            file.write_all(&serde_json::to_vec(&row).map_err(|_| Error::Invalid("F08_REPORT_JSON"))?)?;
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

/// Record only after C has durably stored a trusted successful inclusion.
/// No Receipt lookup or Apply may have occurred at this boundary.
pub fn run<T>(
    engine: &Engine,
    expected_hash: &str,
    directory: &std::path::Path,
    action: impl FnOnce(&Reached) -> Result<T>,
) -> Result<T> {
    schema::validate("Hash", &json!(expected_hash))?;
    let view = engine.reader().get()?;
    let attempt = engine
        .committed_attempt(expected_hash)?
        .ok_or(Error::Invalid("F08_ATTEMPT"))?;
    if attempt["kind"] != "SETTLE"
        || attempt["state"] != "INCLUDED_SUCCESS"
        || attempt["tx_hash"] != expected_hash
        || schema::num(&attempt["confirmed_tx"]["abci_code"])? != 0
        || attempt["confirmed_tx"]["tx_hash"] != expected_hash
    {
        return Err(Error::Invalid("F08_COMMIT_PROOF"));
    }
    let height = schema::num(&attempt["confirmed_tx"]["height"])?;
    let batch_id = attempt["batch"]["batch_id"]
        .as_str()
        .ok_or("F08_BATCH")?;
    schema::validate("Hash", &json!(batch_id))?;
    let receipts = view.state["resolution_receipts"]
        .as_array()
        .ok_or(Error::Invalid("F08_RECEIPTS"))?;
    if receipts.iter().any(|r| r["batch"]["batch_id"] == batch_id)
        || matches!(view.state["batches"][0]["state"].as_str(), Some("COMMITTED" | "CORRECTED"))
    {
        return Err(Error::Recovery("F08_EFFECT_ALREADY_VISIBLE"));
    }
    let boundary = canonical(&json!({
        "schema":"sre-chain-commit-response-boundary/1", "boundary":"F08",
        "tx_hash":expected_hash, "batch_id":batch_id,
        "height":height.to_string(), "abci_code":"0",
        "attempt_state":"INCLUDED_SUCCESS",
        "commit":{"command_seq":view.commit.command_seq.to_string(),
            "record_hash":view.commit.record_hash,"end_offset":view.commit.end_offset.to_string()},
        "response_delivered":false, "receipt_queried":false,
        "asset_effect_visible":false, "reusable_permit":false
    }))?;
    Reached {
        tx_hash: expected_hash.into(),
        batch_id: batch_id.into(),
        height,
        boundary,
    }
    .record(directory, action)
}

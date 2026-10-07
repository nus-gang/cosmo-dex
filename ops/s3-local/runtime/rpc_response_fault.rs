//! Fault-build-only settlement RPC response boundaries (F06/F07).
//! The approved Worker persists SUBMISSION_UNKNOWN before this module invokes
//! the caller's one-shot transport callback.  This module never retries,
//! resolves an Attempt, creates a receipt, or changes economic state.
#![cfg(feature = "fault-injection")]

use base64::Engine as _;
use nus_exchange_contract::s3::{
    dev_local::{Error, Result},
    journal::canonical,
    schema,
    settlement_local::Worker,
    snapshot::Observation,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    BeforeHeaders,
    PartialJson,
}

impl Boundary {
    fn label(self) -> &'static str {
        match self {
            Self::BeforeHeaders => "F06",
            Self::PartialJson => "F07",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportObservation {
    request_sha256: String,
    headers_sha256: Option<String>,
    partial_json_sha256: Option<String>,
    partial_json_len: usize,
}

impl TransportObservation {
    /// The exact request was handed to transport, then the connection ended
    /// before any response header was accepted.
    pub fn before_headers(request: &[u8]) -> Result<Self> {
        if request.is_empty() || request.len() > 139264 {
            return Err(Error::Invalid("RPC_RESPONSE_REQUEST"));
        }
        Ok(Self {
            request_sha256: hex::encode(Sha256::digest(request)),
            headers_sha256: None,
            partial_json_sha256: None,
            partial_json_len: 0,
        })
    }

    /// Complete HTTP headers were accepted, but only a non-empty prefix of the
    /// JSON entity arrived.  The prefix is provenance only and is not decoded.
    pub fn partial_json(request: &[u8], headers: &[u8], partial: &[u8]) -> Result<Self> {
        if request.is_empty()
            || request.len() > 139264
            || headers.is_empty()
            || headers.len() > 16384
            || !headers.ends_with(b"\r\n\r\n")
            || partial.is_empty()
            || partial.len() > 65536
        {
            return Err(Error::Invalid("RPC_RESPONSE_PARTIAL"));
        }
        Ok(Self {
            request_sha256: hex::encode(Sha256::digest(request)),
            headers_sha256: Some(hex::encode(Sha256::digest(headers))),
            partial_json_sha256: Some(hex::encode(Sha256::digest(partial))),
            partial_json_len: partial.len(),
        })
    }
}

pub struct RpcResponseFault {
    hash: String,
    boundary: Boundary,
    claimed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    pub tx_hash: String,
    pub broadcast_count: u64,
    pub boundary: Boundary,
    bytes: Vec<u8>,
}

impl Reached {
    pub fn boundary_sha256(&self) -> String {
        hex::encode(Sha256::digest(&self.bytes))
    }

    pub fn record<T>(
        &self,
        directory: &std::path::Path,
        action: impl FnOnce() -> Result<T>,
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
            return Err(Error::Invalid("RPC_RESPONSE_REPORT_ROOT"));
        }
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(directory)?;
        let rm = root.metadata()?;
        if rm.uid() != unsafe { libc::geteuid() } || rm.mode() & 0o7777 != 0o700 {
            return Err(Error::Invalid("RPC_RESPONSE_REPORT_ROOT"));
        }
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                c"rpc-response.jsonl".as_ptr(),
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
            let current_file = std::fs::symlink_metadata(directory.join("rpc-response.jsonl"))?;
            if (current_root.dev(), current_root.ino(), current_root.mode(), current_root.uid())
                != (rm.dev(), rm.ino(), rm.mode(), rm.uid())
                || (current_file.dev(), current_file.ino(), current_file.mode(), current_file.uid(), current_file.nlink())
                    != (fm.dev(), fm.ino(), fm.mode(), fm.uid(), 1)
                || fm.mode() & 0o7777 != 0o600
            {
                return Err(Error::Invalid("RPC_RESPONSE_REPORT_CHANGED"));
            }
            let row = json!({
                "schema":"s3-local-rpc-response/1", "phase":phase,
                "boundary_base64":base64::engine::general_purpose::STANDARD.encode(&self.bytes),
                "boundary_sha256":self.boundary_sha256(),
                "socket_verified":false,
                "response_complete":false, "attempt_resolved":false,
                "asset_effect_verified":false, "crash_verified":false,
                "durable_ack":false, "DEV":"NOT_RUN"
            });
            file.write_all(&serde_json::to_vec(&row).map_err(|_| Error::Invalid("RPC_RESPONSE_REPORT_JSON"))?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            root.sync_all()?;
            Ok(())
        };
        append("reserved")?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
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

impl RpcResponseFault {
    pub fn new(hash: &str, boundary: Boundary, enable: bool, allow: bool) -> Result<Self> {
        if !enable
            || !allow
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Invalid("RPC_RESPONSE_OPTIONS"));
        }
        Ok(Self { hash: hash.into(), boundary, claimed: false })
    }

    pub fn run<T>(
        &mut self,
        worker: &Worker,
        observation: &Observation,
        now: u64,
        transport: impl FnOnce(&[u8]) -> Result<TransportObservation>,
        directory: &std::path::Path,
        action: impl FnOnce(&Reached) -> Result<T>,
    ) -> Result<T> {
        if self.claimed {
            return Err(Error::Recovery("RPC_RESPONSE_CLOSED"));
        }
        self.claimed = true;
        worker.test_broadcast(&self.hash, observation, now, |stored, raw| {
            let transport = transport(raw)?;
            let reached = self.inspect(stored, raw, transport)?;
            reached.record(directory, || action(&reached))
        })?
    }

    fn inspect(&self, stored: &Value, raw: &[u8], transport: TransportObservation) -> Result<Reached> {
        let count = schema::num(&stored["broadcast_count"])?;
        let raw_sha = hex::encode(Sha256::digest(raw));
        if stored["state"] != "SUBMISSION_UNKNOWN"
            || stored["tx_hash"] != self.hash
            || !(1..=3).contains(&count)
            || raw.is_empty()
            || raw.len() > 139264
            || raw_sha != self.hash
            || transport.request_sha256 != raw_sha
        {
            return Err(Error::Recovery("RPC_RESPONSE_IDENTITY"));
        }
        match self.boundary {
            Boundary::BeforeHeaders if transport.headers_sha256.is_some()
                || transport.partial_json_sha256.is_some()
                || transport.partial_json_len != 0 => return Err(Error::Invalid("RPC_RESPONSE_PHASE")),
            Boundary::PartialJson if transport.headers_sha256.is_none()
                || transport.partial_json_sha256.is_none()
                || transport.partial_json_len == 0 => return Err(Error::Invalid("RPC_RESPONSE_PHASE")),
            _ => {}
        }
        let bytes = canonical(&json!({
            "schema":"sre-rpc-response-boundary/1", "boundary":self.boundary.label(),
            "state":"SUBMISSION_UNKNOWN", "tx_hash":self.hash,
            "broadcast_count":count.to_string(), "raw_len":raw.len().to_string(),
            "stored_intent_sha256":hex::encode(Sha256::digest(canonical(stored)?)),
            "request_sha256":transport.request_sha256,
            "headers_sha256":transport.headers_sha256,
            "partial_json_sha256":transport.partial_json_sha256,
            "partial_json_len":transport.partial_json_len.to_string(),
            "transport_callback_called":true, "socket_verified":false,
            "response_complete":false,
            "attempt_resolved":false, "reusable_permit":false
        }))?;
        Ok(Reached { tx_hash: self.hash.clone(), broadcast_count: count, boundary: self.boundary, bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_observation_rejects_malformed_boundaries() {
        assert!(TransportObservation::before_headers(&[]).is_err());
        assert!(TransportObservation::partial_json(b"tx", b"header", b"{").is_err());
        assert!(TransportObservation::partial_json(b"tx", b"HTTP/1.1 200 OK\r\n\r\n", b"").is_err());
        assert!(TransportObservation::partial_json(b"tx", b"HTTP/1.1 200 OK\r\n\r\n", b"{").is_ok());
    }

    #[test]
    fn options_are_explicit_and_single_use() {
        let hash = "0".repeat(64);
        assert!(RpcResponseFault::new(&hash, Boundary::BeforeHeaders, false, true).is_err());
        assert!(RpcResponseFault::new(&hash, Boundary::BeforeHeaders, true, false).is_err());
        assert!(RpcResponseFault::new(&"A".repeat(64), Boundary::BeforeHeaders, true, true).is_err());
    }
}

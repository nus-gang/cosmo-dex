//! Fault-build only: stop at L-D's committed intent callback, with no transport.
//! A reached callback is not a chain receipt, crash proof, or reusable send permit.
#![cfg(feature = "fault-injection")]
use nus_exchange_contract::s3::journal::canonical;
use nus_exchange_contract::s3::{
    dev_local::{Error, Result},
    schema,
    settlement_local::Worker,
    snapshot::Observation,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub struct BeforeSend {
    hash: String,
    claimed: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Reached {
    pub tx_hash: String,
    pub broadcast_count: u64,
    pub raw_len: usize,
    boundary_bytes: Vec<u8>,
}
impl Reached {
    /// Canonical provenance captured inside the committed-intent callback.
    /// Contains hashes, never TX bytes or the full stored intent. Not a permit.
    pub fn boundary_bytes(&self) -> &[u8] {
        &self.boundary_bytes
    }
    pub fn boundary_sha256(&self) -> String {
        hex::encode(Sha256::digest(&self.boundary_bytes))
    }
}
impl Reached {
    /// Call inside BeforeSend::run's writer-locked callback. The reservation
    /// follows durable intent; it does not precede or authorize intent creation.
    /// Missing/partial final is UNKNOWN, even after an observed child exit.
    pub fn record_boundary<T>(
        &self,
        directory: &std::path::Path,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.record_with_sync(directory, action, |f| f.sync_all())
    }
    fn record_with_sync<T>(
        &self,
        directory: &std::path::Path,
        action: impl FnOnce() -> Result<T>,
        mut sync: impl FnMut(&std::fs::File) -> std::io::Result<()>,
    ) -> Result<T> {
        use base64::Engine as _;
        use std::{
            fs::{File, OpenOptions},
            io::Write,
            os::{
                fd::{AsRawFd, FromRawFd},
                unix::fs::{MetadataExt, OpenOptionsExt},
            },
        };
        if !directory.is_absolute() || directory.canonicalize()? != directory {
            return Err(Error::Invalid("F05_REPORT_ROOT"));
        }
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(directory)?;
        let rm = root.metadata()?;
        if rm.uid() != unsafe { libc::geteuid() } || rm.mode() & 0o7777 != 0o700 {
            return Err(Error::Invalid("F05_REPORT_ROOT"));
        }
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                c"before-send.jsonl".as_ptr(),
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
            let check = || -> Result<()> {
                let r = std::fs::symlink_metadata(directory)?;
                let f = std::fs::symlink_metadata(directory.join("before-send.jsonl"))?;
                if (r.dev(), r.ino(), r.mode(), r.uid())
                    != (rm.dev(), rm.ino(), rm.mode(), rm.uid())
                    || (f.dev(), f.ino(), f.mode(), f.uid(), f.nlink())
                        != (fm.dev(), fm.ino(), fm.mode(), fm.uid(), 1)
                    || fm.mode() & 0o7777 != 0o600
                {
                    return Err(Error::Invalid("F05_REPORT_CHANGED"));
                }
                Ok(())
            };
            check()?;
            let row = json!({"schema":"s3-local-before-send/1","phase":phase,
                "boundary_base64":base64::engine::general_purpose::STANDARD.encode(self.boundary_bytes()),
                "boundary_sha256":self.boundary_sha256(),"transport_called":false,
                "crash_verified":false,"command_success_verified":false,
                "durable_ack":false,"DEV":"NOT_RUN"});
            file.write_all(
                &serde_json::to_vec(&row).map_err(|_| Error::Invalid("F05_REPORT_JSON"))?,
            )?;
            file.write_all(b"\n")?;
            sync(&file)?;
            sync(&root)?;
            check()
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
            Err(p) => std::panic::resume_unwind(p),
            Ok(r) => {
                persisted?;
                r
            }
        }
    }
}

impl BeforeSend {
    pub fn new(hash: &str, enable: bool, allow: bool) -> Result<Self> {
        if !enable
            || !allow
            || hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Invalid("BEFORE_SEND_OPTIONS"));
        }
        Ok(Self {
            hash: hash.into(),
            claimed: false,
        })
    }
    /// Invoke only on an isolated fault-build worker. L-D owns intent persistence,
    /// retry limits and C's writer-lock callback. No socket function is supplied.
    /// Once called, errors/unwind also consume this driver; never retry it.
    pub fn run(
        &mut self,
        worker: &Worker,
        o: &Observation,
        now: u64,
        boundary: impl FnOnce(&Reached) -> Result<()>,
    ) -> Result<Reached> {
        self.claim()?;
        worker.test_broadcast(&self.hash, o, now, |stored, raw| {
            self.inspect(stored, raw, boundary)
        })?
    }
    fn claim(&mut self) -> Result<()> {
        if self.claimed {
            return Err(Error::Recovery("BEFORE_SEND_CLOSED"));
        }
        self.claimed = true;
        Ok(())
    }
    fn inspect(
        &self,
        stored: &Value,
        raw: &[u8],
        boundary: impl FnOnce(&Reached) -> Result<()>,
    ) -> Result<Reached> {
        let count = schema::num(&stored["broadcast_count"])?;
        if stored["state"] != "SUBMISSION_UNKNOWN"
            || stored["tx_hash"] != self.hash
            || !(1..=3).contains(&count)
            || raw.is_empty()
            || raw.len() > 139264
            || hex::encode(Sha256::digest(raw)) != self.hash
        {
            return Err(Error::Recovery("BEFORE_SEND_IDENTITY"));
        }
        let boundary_bytes = canonical(&json!({
            "schema":"sre-before-send-boundary/1", "boundary":"F05",
            "state":"SUBMISSION_UNKNOWN", "tx_hash":self.hash,
            "broadcast_count":count.to_string(), "raw_len":raw.len().to_string(),
            "stored_intent_sha256":hex::encode(Sha256::digest(canonical(stored)?)),
            "transport_called":false, "crash_verified":false,
            "reusable_permit":false
        }))?;
        let reached = Reached {
            tx_hash: self.hash.clone(),
            broadcast_count: count,
            raw_len: raw.len(),
            boundary_bytes,
        };
        boundary(&reached)?;
        Ok(reached)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (BeforeSend, Value, Vec<u8>) {
        let raw = b"synthetic-tx".to_vec();
        let hash = hex::encode(Sha256::digest(&raw));
        let s = json!({"state":"SUBMISSION_UNKNOWN","tx_hash":hash,"broadcast_count":"1"});
        (BeforeSend::new(&hash, true, true).unwrap(), s, raw)
    }

    fn report_root(tag: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("f05-report-{}-{tag}", std::process::id()));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p.canonicalize().unwrap()
    }
    #[test]
    fn report_sync_order_exact_bytes_and_error_or_panic() {
        use base64::Engine as _;
        use std::{cell::Cell, os::unix::fs::MetadataExt};
        for mode in 0..3 {
            let p = report_root(&format!("outcome-{mode}"));
            let (d, s, raw) = fixture();
            let r = d.inspect(&s, &raw, |_| Ok(())).unwrap();
            let syncs = Cell::new(0);
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                r.record_with_sync(
                    &p,
                    || -> Result<()> {
                        assert_eq!(syncs.get(), 2);
                        if mode == 2 {
                            panic!("expected")
                        };
                        if mode == 1 {
                            return Err(Error::Invalid("STOP"));
                        };
                        Ok(())
                    },
                    |f| {
                        syncs.set(syncs.get() + 1);
                        f.sync_all()
                    },
                )
            }));
            assert_eq!(out.is_err(), mode == 2);
            if mode != 2 {
                assert_eq!(out.unwrap().is_err(), mode == 1)
            }
            assert_eq!(syncs.get(), 4);
            let file = p.join("before-send.jsonl");
            assert_eq!(std::fs::metadata(&file).unwrap().mode() & 0o7777, 0o600);
            let rows: Vec<Value> = std::fs::read_to_string(file)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0]["phase"], "reserved");
            assert_eq!(
                rows[1]["phase"],
                ["boundary_returned", "boundary_error", "panic"][mode]
            );
            for row in rows {
                assert_eq!(row["boundary_sha256"], r.boundary_sha256());
                assert_eq!(
                    base64::engine::general_purpose::STANDARD
                        .decode(row["boundary_base64"].as_str().unwrap())
                        .unwrap(),
                    r.boundary_bytes()
                );
                assert_eq!(row["crash_verified"], false);
                assert_eq!(row["transport_called"], false);
            }
            assert!(r.record_boundary::<()>(&p, || panic!("retry")).is_err());
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn reservation_sync_failure_prevents_action_and_preserves_file() {
        for fail in 1..=2 {
            let p = report_root(&format!("sync-{fail}"));
            let (d, s, raw) = fixture();
            let r = d.inspect(&s, &raw, |_| Ok(())).unwrap();
            let mut calls = 0;
            assert!(
                r.record_with_sync::<()>(
                    &p,
                    || panic!("action"),
                    |f| {
                        calls += 1;
                        if calls == fail {
                            Err(std::io::Error::other("sync"))
                        } else {
                            f.sync_all()
                        }
                    }
                )
                .is_err()
            );
            assert!(p.join("before-send.jsonl").exists());
            assert!(r.record_boundary::<()>(&p, || panic!("retry")).is_err());
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn replacement_and_final_sync_failures_are_not_success() {
        for mode in 0..2 {
            let p = report_root(&format!("changed-{mode}"));
            let (d, s, raw) = fixture();
            let r = d.inspect(&s, &raw, |_| Ok(())).unwrap();
            let mut calls = 0;
            let out = r.record_with_sync(
                &p,
                || {
                    if mode == 0 {
                        std::fs::rename(p.join("before-send.jsonl"), p.join("preserved")).unwrap();
                        std::fs::write(p.join("before-send.jsonl"), b"replacement").unwrap();
                    }
                    Ok(())
                },
                |f| {
                    calls += 1;
                    if mode == 1 && calls == 3 {
                        Err(std::io::Error::other("final sync"))
                    } else {
                        f.sync_all()
                    }
                },
            );
            assert!(out.is_err());
            assert!(p.join("before-send.jsonl").exists());
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn exact_identity_and_single_claim() {
        let (mut d, s, raw) = fixture();
        d.claim().unwrap();
        let mut calls = 0;
        let r = d
            .inspect(&s, &raw, |r| {
                calls += 1;
                assert_eq!(r.broadcast_count, 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(r.raw_len, raw.len());
        assert_eq!(calls, 1);
        assert!(d.claim().is_err());
    }
    #[test]
    fn malformed_identity_never_reaches_boundary() {
        let (d, s, raw) = fixture();
        for (key, value) in [
            ("state", json!("PREPARED")),
            ("tx_hash", json!("0".repeat(64))),
            ("broadcast_count", json!("0")),
            ("broadcast_count", json!("4")),
            ("broadcast_count", json!("01")),
        ] {
            let mut bad = s.clone();
            bad[key] = value;
            assert!(
                d.inspect(&bad, &raw, |_| panic!("boundary reached"))
                    .is_err()
            );
        }
        for bad in [Vec::new(), b"changed".to_vec(), vec![0; 139265]] {
            assert!(d.inspect(&s, &bad, |_| panic!("boundary reached")).is_err());
        }
    }
    #[test]
    fn boundary_provenance_is_canonical_owned_and_binds_exact_intent() {
        let (d, mut stored, raw) = fixture();
        stored["private_fixture_only"] = json!("must-not-be-exposed");
        let reached = d
            .inspect(&stored, &raw, |r| {
                assert_eq!(
                    r.boundary_sha256(),
                    hex::encode(Sha256::digest(r.boundary_bytes()))
                );
                Ok(())
            })
            .unwrap();
        let bytes = reached.boundary_bytes().to_vec();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(canonical(&v).unwrap(), bytes);
        assert_eq!(
            v["stored_intent_sha256"],
            hex::encode(Sha256::digest(canonical(&stored).unwrap()))
        );
        assert_eq!(v["tx_hash"], reached.tx_hash);
        assert_eq!(v["broadcast_count"], "1");
        assert_eq!(v["transport_called"], false);
        assert!(
            !String::from_utf8(bytes.clone())
                .unwrap()
                .contains("must-not-be-exposed")
        );
        let same = d.inspect(&stored, &raw, |_| Ok(())).unwrap();
        assert_eq!(same.boundary_bytes(), bytes);
        stored["broadcast_count"] = json!("2");
        let changed = d.inspect(&stored, &raw, |_| Ok(())).unwrap();
        assert_ne!(changed.boundary_sha256(), reached.boundary_sha256());
        assert_eq!(reached.boundary_bytes(), bytes);
        stored["private_fixture_only"] = json!("changed");
        assert_ne!(
            d.inspect(&stored, &raw, |_| Ok(()))
                .unwrap()
                .boundary_sha256(),
            changed.boundary_sha256()
        );
    }
    #[test]
    fn options_error_and_unwind_keep_driver_consumed() {
        for h in ["", "0", &"A".repeat(64)] {
            assert!(BeforeSend::new(h, true, true).is_err());
        }
        for flags in [(false, true), (true, false)] {
            assert!(BeforeSend::new(&"0".repeat(64), flags.0, flags.1).is_err());
        }
        let (mut d, s, raw) = fixture();
        d.claim().unwrap();
        assert!(
            d.inspect(&s, &raw, |_| Err(Error::Invalid("BOUNDARY_STOP")))
                .is_err()
        );
        assert!(d.claim().is_err());
        let (mut d, s, raw) = fixture();
        d.claim().unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.inspect(
                &s,
                &raw,
                |_| panic!("stop")
            )))
            .is_err()
        );
        assert!(d.claim().is_err());
    }
}

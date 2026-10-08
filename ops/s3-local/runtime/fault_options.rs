//! Explicit options for the separate, one-command storage fault child.
use nus_exchange_contract::s3::dev_local::{Error, Result};
use std::path::PathBuf;
#[path = "storage_fault.rs"]
mod storage_fault;
pub struct Options {
    pub errno: Option<String>,
    pub point: String,
    pub occurrence: u32,
    pub purpose: String,
    pub evidence: PathBuf,
}
impl Options {
    pub fn parse(args: &mut std::iter::Peekable<impl Iterator<Item = String>>) -> Result<Self> {
        let bad = || Error::Invalid("FAULT_OPTIONS");
        if args.next().as_deref() != Some("--enable-storage-fault") {
            return Err(bad());
        }
        let mut take = |name: &str| -> Result<String> {
            if args.next().as_deref() != Some(name) {
                return Err(bad());
            }
            args.next().ok_or_else(bad)
        };
        let point = take("--fault-point")?;
        let raw = take("--fault-occurrence")?;
        let occurrence = raw.parse::<u32>().map_err(|_| bad())?;
        if occurrence.to_string() != raw {
            return Err(bad());
        }
        storage_fault::StorageFault::new(&point, occurrence, true, true)?;
        let purpose = take("--fault-purpose")?;
        if !["NORMAL", "RESOLVE_FAILURE"].contains(&purpose.as_str()) {
            return Err(bad());
        }
        let evidence = PathBuf::from(take("--fault-evidence-root")?);
        if !evidence.is_absolute()
            || evidence.components().any(|x| {
                matches!(
                    x,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(bad());
        }
        let errno = if args.peek().map(String::as_str) == Some("--fault-errno") {
            args.next();
            let value = args.next().ok_or_else(bad)?;
            if !["ENOSPC", "EDQUOT", "EIO"].contains(&value.as_str()) {
                return Err(bad());
            }
            Some(value)
        } else {
            None
        };
        Ok(Self {
            errno,
            point,
            occurrence,
            purpose,
            evidence,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args() -> Vec<String> {
        [
            "--enable-storage-fault",
            "--fault-point",
            "before_wal",
            "--fault-occurrence",
            "1",
            "--fault-purpose",
            "NORMAL",
            "--fault-evidence-root",
            "/private/evidence",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    #[test]
    fn explicit_exact_options_leave_worker_arguments() {
        for p in ["NORMAL", "RESOLVE_FAILURE"] {
            let mut a = args();
            a[6] = p.into();
            a.push("--input-set".into());
            let mut i = a.into_iter().peekable();
            let o = Options::parse(&mut i).unwrap();
            assert_eq!(o.purpose, p);
            assert_eq!(o.point, "before_wal");
            assert_eq!(o.occurrence, 1);
            assert_eq!(i.next().as_deref(), Some("--input-set"));
        }
    }
    #[test]
    fn errno_is_exact_optional_and_preserves_worker_arguments() {
        for value in ["ENOSPC", "EDQUOT", "EIO"] {
            let mut a = args();
            a.extend(["--fault-errno", value, "--input-set"].map(str::to_owned));
            let mut i = a.into_iter().peekable();
            assert_eq!(
                Options::parse(&mut i).unwrap().errno.as_deref(),
                Some(value)
            );
            assert_eq!(i.next().as_deref(), Some("--input-set"));
        }
        for value in ["Generic", "enospc", "28", "--input-set", ""] {
            let mut a = args();
            a.extend(["--fault-errno", value].map(str::to_owned));
            assert!(Options::parse(&mut a.into_iter().peekable()).is_err());
        }
        assert!(
            Options::parse(&mut args().into_iter().peekable())
                .unwrap()
                .errno
                .is_none()
        );
    }
    #[test]
    fn missing_noncanonical_unknown_and_unbounded_options_reject() {
        for len in 0..9 {
            assert!(Options::parse(&mut args()[..len].to_vec().into_iter().peekable()).is_err());
        }
        for (index, value) in [
            (0, "--enable"),
            (1, "--fault-po"),
            (2, "not-a-hook"),
            (4, "0"),
            (4, "01"),
            (4, "1025"),
            (4, "-1"),
            (4, "4294967296"),
            (6, "AUTO"),
            (8, "relative"),
            (8, "/a/../b"),
        ] {
            let mut a = args();
            a[index] = value.into();
            assert!(Options::parse(&mut a.into_iter().peekable()).is_err());
        }
    }
}

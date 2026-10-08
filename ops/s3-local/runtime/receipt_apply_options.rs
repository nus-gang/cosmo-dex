//! Pure options for the dedicated F09 Receipt->Apply crash child.
use nus_exchange_contract::s3::dev_local::{Error, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub struct Options {
    pub batch_id: String,
    pub evidence: PathBuf,
}
impl Options {
    pub fn parse(args: &mut impl Iterator<Item = String>) -> Result<Self> {
        let bad = || Error::Invalid("F09_OPTIONS");
        let mut take = |name: &str| -> Result<String> {
            if args.next().as_deref() != Some(name) {
                return Err(bad());
            }
            args.next().ok_or_else(bad)
        };
        if take("--enable-f09-receipt-apply")? != "true" {
            return Err(bad());
        }
        let batch_id = take("--batch-id")?;
        if batch_id.len() != 64
            || !batch_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(bad());
        }
        let raw = take("--fault-evidence-root")?;
        let evidence = PathBuf::from(&raw);
        if !evidence.is_absolute()
            || raw.contains('\0')
            || raw
                .split('/')
                .skip(1)
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(bad());
        }
        if args.next().as_deref() != Some("--worker-inputs") {
            return Err(bad());
        }
        Ok(Self { batch_id, evidence })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn argv() -> Vec<String> {
        vec![
            "--enable-f09-receipt-apply".into(),
            "true".into(),
            "--batch-id".into(),
            "ab".repeat(32),
            "--fault-evidence-root".into(),
            "/private/f09 evidence".into(),
            "--worker-inputs".into(),
            "--input-set".into(),
            "/private/input".into(),
        ]
    }
    #[test]
    fn exact_input_preserves_worker_tail() {
        let mut args = argv().into_iter();
        let value = Options::parse(&mut args).unwrap();
        assert_eq!(value.batch_id, "ab".repeat(32));
        assert_eq!(value.evidence, PathBuf::from("/private/f09 evidence"));
        assert_eq!(args.collect::<Vec<_>>(), ["--input-set", "/private/input"]);
    }
    #[test]
    fn mixed_modes_and_noncanonical_values_reject() {
        for (i, value) in [
            (0, "--enable-f05-before-send"),
            (1, "false"),
            (2, "--tx-hash"),
            (4, "--evidence-root"),
            (6, "--input-set"),
        ] {
            let mut a = argv();
            a[i] = value.into();
            assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        for extra in [
            vec!["--fault-errno", "EIO"],
            vec!["--fault-command", "Apply"],
            vec!["--fault-occurrence", "1"],
            vec!["--enable-f14-prepare", "true"],
        ] {
            let mut a = argv();
            a.splice(6..6, extra.into_iter().map(str::to_owned));
            assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        for batch in [
            String::new(),
            "a".repeat(63),
            "A".repeat(64),
            "g".repeat(64),
        ] {
            let mut a = argv();
            a[3] = batch;
            assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        for path in [
            "relative",
            "/",
            "/private/../f09",
            "//private/f09",
            "/private/f09/",
        ] {
            let mut a = argv();
            a[5] = path.into();
            assert!(Options::parse(&mut a.into_iter()).is_err());
        }
    }
}

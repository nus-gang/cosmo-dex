//! Pure input boundary for the dedicated F14 Prepare diagnostic child.
//! Parsing grants no approval and installs no hook. Global dev opt-ins and
//! authenticated START remain the executable owner's responsibility.
use nus_exchange_contract::s3::dev_local::{Error, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub struct Options { pub occurrence: u32, pub evidence: PathBuf }
impl Options {
    pub fn parse(args: &mut impl Iterator<Item=String>) -> Result<Self> {
        let bad=||Error::Invalid("F14_OPTIONS");
        let mut take=|name: &str| -> Result<String> {
            if args.next().as_deref()!=Some(name) { return Err(bad()); }
            args.next().ok_or_else(bad)
        };
        // Fixed phase: SemanticReplay must never be an injectable CLI choice.
        if take("--enable-f14-prepare")? != "true" { return Err(bad()); }
        let raw=take("--fault-occurrence")?;
        let occurrence=raw.parse::<u32>().map_err(|_|bad())?;
        if !(1..=1024).contains(&occurrence) || occurrence.to_string()!=raw { return Err(bad()); }
        let raw_path=take("--fault-evidence-root")?;
        let evidence=PathBuf::from(&raw_path);
        if !evidence.is_absolute() || raw_path.contains('\0') ||
            raw_path.split('/').skip(1).any(|part| part.is_empty() || part=="." || part=="..") {
            return Err(bad());
        }
        if args.next().as_deref()!=Some("--worker-inputs") { return Err(bad()); }
        Ok(Self {occurrence,evidence})
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn argv()->Vec<String> { ["--enable-f14-prepare","true","--fault-occurrence","1",
        "--fault-evidence-root","/private/evidence","--worker-inputs","--input-set","/private/input"]
        .into_iter().map(str::to_owned).collect() }
    #[test] fn exact_bounds_preserve_all_worker_arguments() {
        for occurrence in [1,1024] {
            let mut a=argv(); a[3]=occurrence.to_string(); let mut args=a.into_iter();
            let value=Options::parse(&mut args).unwrap();
            assert_eq!(value.occurrence,occurrence);
            assert_eq!(value.evidence,PathBuf::from("/private/evidence"));
            assert_eq!(args.collect::<Vec<_>>(),["--input-set","/private/input"]);
        }
    }
    #[test] fn opt_in_phase_errno_command_and_delimiter_mixing_reject() {
        for (i,value) in [(0,"--enable-storage-fault"),(0,"--enable-storage-crash"),
            (0,"--enable-f14"),(1,"false"),(1,"1"),(1,"TRUE"),(2,"--fault-phase"),
            (6,"--fault-errno"),(6,"--fault-purpose"),(6,"--fault-command"),(6,"--input-set")] {
            let mut a=argv(); a[i]=value.into();
            assert!(matches!(Options::parse(&mut a.into_iter()),Err(Error::Invalid("F14_OPTIONS"))));
        }
        for extra in [vec!["--fault-phase","SemanticReplay"],vec!["--fault-errno","EIO"],
            vec!["--fault-command","Seal"],vec!["--fault-occurrence","2"]] {
            let mut a=argv(); a.splice(6..6,extra.into_iter().map(str::to_owned));
            assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        for len in 0..7 { assert!(Options::parse(&mut argv()[..len].to_vec().into_iter()).is_err()); }
    }
    #[test] fn noncanonical_numbers_and_paths_reject_without_io() {
        for value in ["0","1025","01","+1","-1"," 1","1 ","4294967296",""] {
            let mut a=argv(); a[3]=value.into(); assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        for value in ["relative","/","/private/../evidence","/private/./evidence",
            "//private/evidence","/private//evidence","/private/evidence/","/private/\0evidence"] {
            let mut a=argv(); a[5]=value.into(); assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        // Existence is deliberately not consulted by this pure parser.
        let mut a=argv(); a[5]="/does-not-exist/f14 evidence".into();
        assert!(Options::parse(&mut a.into_iter()).is_ok());
    }
}

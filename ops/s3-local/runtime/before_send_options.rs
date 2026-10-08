//! Pure options for a dedicated F05 diagnostic child. No IO or send permit.
//! The child must still enforce dev opt-ins, captured inputs and authenticated START.
use nus_exchange_contract::s3::dev_local::{Error, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub struct Options { pub tx_hash: String, pub evidence: PathBuf }
impl Options {
    pub fn parse(args: &mut impl Iterator<Item=String>) -> Result<Self> {
        let bad=||Error::Invalid("F05_OPTIONS");
        let mut take=|name: &str| -> Result<String> {
            if args.next().as_deref()!=Some(name) {return Err(bad());}
            args.next().ok_or_else(bad)
        };
        if take("--enable-f05-before-send")? != "true" {return Err(bad());}
        let tx_hash=take("--tx-hash")?;
        if tx_hash.len()!=64 || !tx_hash.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(bad());
        }
        let raw=take("--fault-evidence-root")?;
        let evidence=PathBuf::from(&raw);
        if !evidence.is_absolute() || raw.contains('\0') ||
            raw.split('/').skip(1).any(|p|p.is_empty() || p=="." || p=="..") {return Err(bad());}
        if args.next().as_deref()!=Some("--worker-inputs") {return Err(bad());}
        Ok(Self {tx_hash,evidence})
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn argv()->Vec<String> {
        vec!["--enable-f05-before-send".into(),"true".into(),"--tx-hash".into(),"ab".repeat(32),
            "--fault-evidence-root".into(),"/private/f05 evidence".into(),"--worker-inputs".into(),
            "--input-set".into(),"/private/input".into()]
    }
    #[test] fn exact_hash_path_and_worker_tail_are_preserved() {
        let mut args=argv().into_iter();let options=Options::parse(&mut args).unwrap();
        assert_eq!(options.tx_hash,"ab".repeat(32));
        assert_eq!(options.evidence,PathBuf::from("/private/f05 evidence"));
        assert_eq!(args.collect::<Vec<_>>(),["--input-set","/private/input"]);
    }
    #[test] fn opt_in_and_other_fault_modes_cannot_be_mixed() {
        for (i,value) in [(0,"--enable-storage-crash"),(0,"--enable-f14-prepare"),(1,"false"),
            (1,"TRUE"),(1,"1"),(2,"--tx-bytes"),(4,"--evidence-root"),(6,"--input-set")] {
            let mut a=argv();a[i]=value.into();
            assert!(matches!(Options::parse(&mut a.into_iter()),Err(Error::Invalid("F05_OPTIONS"))));
        }
        for extra in [vec!["--fault-errno","EIO"],vec!["--fault-command","Apply"],
            vec!["--fault-phase","SemanticReplay"],vec!["--tx-hash","duplicate"],
            vec!["--fault-occurrence","2"]] {
            let mut a=argv();a.splice(6..6,extra.into_iter().map(str::to_owned));
            assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        for len in 0..7 {assert!(Options::parse(&mut argv()[..len].to_vec().into_iter()).is_err());}
    }
    #[test] fn noncanonical_hash_and_path_are_rejected_without_io() {
        for hash in [String::new(),"a".repeat(63),"a".repeat(65),"A".repeat(64),
            "g".repeat(64),format!(" {}","a".repeat(63)),"é".repeat(32)] {
            let mut a=argv();a[3]=hash;assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        for path in ["relative","/","/private/../f05","/private/./f05","//private/f05",
            "/private//f05","/private/f05/","/private/\0f05"] {
            let mut a=argv();a[5]=path.into();assert!(Options::parse(&mut a.into_iter()).is_err());
        }
        let mut a=argv();a[5]="/not-created/f05".into();assert!(Options::parse(&mut a.into_iter()).is_ok());
    }
}

//! Input boundary for the future separate crash child. Parsing never installs a hook.
//! Global dev opt-ins, approval and START remain the executable owner's responsibility.
#[path="fault_options.rs"] mod fault_options;
use nus_exchange_contract::s3::dev_local::{Error, Result};
use std::path::PathBuf;

pub struct Options {
    pub point: String, pub occurrence: u32, pub purpose: String, pub evidence: PathBuf,
}
impl Options {
    pub fn parse(args: &mut std::iter::Peekable<impl Iterator<Item=String>>) -> Result<Self> {
        let bad=||Error::Invalid("CRASH_OPTIONS");
        if args.next().as_deref()!=Some("--enable-storage-crash") {return Err(bad());}
        // Share the exact storage-point/occurrence/purpose/path contract. Only the
        // explicit crash flag is translated; no environment or IO fallback.
        let mut translated=std::iter::once("--enable-storage-fault".to_owned())
            .chain(args.by_ref()).peekable();
        let value=fault_options::Options::parse(&mut translated).map_err(|_|bad())?;
        // The shared parser peeks after the evidence path. Preserve that token:
        // Peekable::peek owns it, so dropping translated would consume worker argv.
        let next=translated.next();
        drop(translated);
        if value.errno.is_some() {return Err(bad());}
        // Require an explicit delimiter rather than accepting an unknown flag or
        // silently swallowing the first worker argument.
        if next.as_deref()!=Some("--worker-inputs") {return Err(bad());}
        Ok(Self {point:value.point,occurrence:value.occurrence,
            purpose:value.purpose,evidence:value.evidence})
    }
}
#[cfg(test)] mod tests {
    use super::*;
    fn argv()->Vec<String> { ["--enable-storage-crash","--fault-point","before_wal",
        "--fault-occurrence","1","--fault-purpose","NORMAL","--fault-evidence-root",
        "/private/evidence","--worker-inputs","--input-set","/private/input"]
        .into_iter().map(str::to_owned).collect() }
    #[test] fn exact_crash_options_preserve_worker_arguments() {
        for purpose in ["NORMAL","RESOLVE_FAILURE"] {
            let mut a=argv();a[6]=purpose.into();let mut it=a.into_iter().peekable();
            let o=Options::parse(&mut it).unwrap();
            assert_eq!((o.point.as_str(),o.occurrence,o.purpose.as_str()),("before_wal",1,purpose));
            assert_eq!(o.evidence,PathBuf::from("/private/evidence"));
            assert_eq!(it.collect::<Vec<_>>(),["--input-set","/private/input"]);
        }
    }
    #[test] fn io_flags_errno_and_missing_boundary_reject() {
        for flag in ["--enable-storage-fault","--enable-storage","--enable-storage-crash=true"] {
            let mut a=argv();a[0]=flag.into();assert!(Options::parse(&mut a.into_iter().peekable()).is_err());
        }
        for errno in ["ENOSPC","EDQUOT","EIO","GENERIC",""] {
            let mut a=argv();a.splice(9..9,["--fault-errno".to_owned(),errno.to_owned()]);
            assert!(Options::parse(&mut a.into_iter().peekable()).is_err());
        }
        for len in 0..10 {assert!(Options::parse(&mut argv()[..len].to_vec().into_iter().peekable()).is_err());}
        let mut a=argv();a.remove(9);assert!(Options::parse(&mut a.into_iter().peekable()).is_err());
    }
    #[test] fn invalid_selector_purpose_path_and_duplicate_reject() {
        for (i,v) in [(2,"unknown"),(4,"0"),(4,"01"),(4,"1025"),(6,"AUTO"),
            (8,"relative"),(8,"/private/../evidence"),(9,"--fault-point")] {
            let mut a=argv();a[i]=v.into();assert!(Options::parse(&mut a.into_iter().peekable()).is_err());
        }
    }
}

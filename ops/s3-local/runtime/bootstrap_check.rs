//! Offline pre-create semantic check. No Engine, signer, home, RPC or listener.
#[path = "capture_io.rs"] mod capture_io;
use nus_exchange_contract::s3::dev_local::{Error, Result, Validated};
use std::{collections::BTreeMap, io::{Read, Write}, path::PathBuf};

pub struct Inputs { profile: PathBuf, pin: String, capture: String }
impl Inputs {
    pub fn parse(args: impl IntoIterator<Item=String>) -> Result<Self> {
        let mut args=args.into_iter();
        if args.next().as_deref()!=Some("validate-captured") { return Err(Error::Invalid("MODE")); }
        let mut flags=BTreeMap::new(); let mut ack=false;
        while let Some(k)=args.next() {
            if k=="--acknowledge-unproven-space" {
                if ack { return Err(Error::Invalid("DUPLICATE_OPTION")); }
                ack=true; continue;
            }
            if !["--capture-sha256","--runtime-pin","--local-demo-profile"].contains(&k.as_str()) {
                return Err(Error::Invalid("UNKNOWN_OPTION"));
            }
            let v=args.next().ok_or(Error::Invalid("OPTION_VALUE"))?;
            if v.is_empty() || v.starts_with("--") || flags.insert(k,v).is_some() {
                return Err(Error::Invalid("OPTION_VALUE"));
            }
        }
        if !ack { return Err(Error::Invalid("LOCAL_DEMO_OPT_IN_REQUIRED")); }
        let mut get=|k:&str| flags.remove(k).ok_or(Error::Invalid("OPTION_REQUIRED"));
        let profile=PathBuf::from(get("--local-demo-profile")?);
        if !profile.is_absolute() || profile.components().any(|c| matches!(c,std::path::Component::ParentDir)) {
            return Err(Error::Invalid("INPUT_PATH"));
        }
        let pin=get("--runtime-pin")?; let capture=get("--capture-sha256")?;
        for h in [&pin,&capture] {
            if h.len()!=64 || !h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(Error::Invalid("INPUT_HASH"));
            }
        }
        Ok(Self {profile,pin,capture})
    }
    pub fn validate(self, reader: impl Read) -> Result<Validated> {
        let raw=capture_io::read_capture(reader,&self.capture)?;
        Validated::decode_bundle(&raw,capture_io::read_regular(&self.profile,1024*1024)?,self.pin,true)
    }
}
fn main() -> std::process::ExitCode {
    let result=Inputs::parse(std::env::args().skip(1)).and_then(|i| i.validate(std::io::stdin().lock()));
    if result.is_err() {
        eprintln!("LOCAL_BOOTSTRAP_CHECK_REJECTED");
        return std::process::ExitCode::from(2);
    }
    let report=b"{\"semantic_validation\":true,\"approval_verified\":false,\"service_started\":false,\"durable_ack\":false}\n";
    if std::io::stdout().lock().write_all(report).is_err() { return std::process::ExitCode::from(2); }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
#[path="../../../exchange/tests/support/dev_fixture.rs"] mod fixture;
#[cfg(test)] mod tests {
    use super::*;
    use nus_exchange_contract::s3::journal::{canonical,sha256};
    fn args(profile:&std::path::Path,pin:&str,hash:&str)->Vec<String> {
        ["validate-captured","--capture-sha256",hash,"--runtime-pin",pin,
            "--local-demo-profile",profile.to_str().unwrap(),"--acknowledge-unproven-space"].into_iter().map(String::from).collect()
    }
    #[test] fn real_descriptor_capture_checker_before_home() {
        let executable=std::env::var("NUS73_BOOTSTRAP_CHECK").expect("checker required");
        let module=std::env::var("NUS73_PROCESS_CHECK_MODULE").unwrap();
        let python=std::env::var("NUS73_PROCESS_CHECK_PYTHON").unwrap();
        let inventory=serde_json::json!({"bin/s3-local-bootstrap-check":sha256(&std::fs::read(&executable).unwrap())});
        for bps in [0,25] {
            let (mut input,_)=fixture::initial(bps);
            for name in ["chain","exchange","settlement","wallet","sre"] {
                let path=format!("chain/local-demo/components/{name}.json");
                let mut descriptor:serde_json::Value=serde_json::from_slice(&input.files[&path]).unwrap();
                descriptor["implementation_settings"]["artifacts_sha256_json"]=serde_json::json!(
                    String::from_utf8(canonical(&inventory).unwrap()).unwrap());
                input.files.insert(path,canonical(&descriptor).unwrap());
            }
            let contract=fixture::aggregate(&input.files);
            let mut manifest:serde_json::Value=serde_json::from_slice(&input.runtime_manifest).unwrap();
            manifest["contract_sha256"]=serde_json::json!(contract);
            manifest["files_sha256"]=serde_json::json!(input.files.iter().map(|(p,b)|(p.clone(),sha256(b))).collect::<std::collections::BTreeMap<_,_>>());
            input.runtime_manifest=canonical(&manifest).unwrap();
            input.approved_runtime_sha256=sha256(&input.runtime_manifest);
            let mut genesis:serde_json::Value=serde_json::from_slice(&input.genesis).unwrap();
            genesis["app_state"]["contract_hash"]=serde_json::json!(contract);
            input.genesis=canonical(&genesis).unwrap();
            let mut guard:serde_json::Value=serde_json::from_slice(&input.guard).unwrap();
            guard["runtime_manifest_sha256"]=serde_json::json!(input.approved_runtime_sha256);
            guard["context"]["contract_hash"]=serde_json::json!(contract);
            guard["context"]["genesis_hash"]=serde_json::json!(sha256(&input.genesis));
            input.guard=canonical(&guard).unwrap();
            let home=fixture::home(bps);
            let root=home.parent().unwrap().join("checker-integration");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("input.json"),canonical(&fixture::bundle(&input)).unwrap()).unwrap();
            std::fs::write(root.join("profile"),&input.effective_profile).unwrap();
            let output=std::process::Command::new(&python)
                .arg(std::path::Path::new(&module).join("test_real_bootstrap_check.py"))
                .arg(&root).arg(&executable).output().unwrap();
            assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
            let report:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["home_created"],false);
            assert_eq!(report["semantic_preflight"]["semantic_validation"],true);
            assert!(!home.exists());
            std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
        }
    }
    #[test] fn real_descriptor_capture_creator_ready_before_home() {
        creator_fixture("test_real_bootstrap_ready.py");
    }
    #[test] fn real_initialize_composition_denies_before_start() {
        creator_fixture("test_real_bootstrap_initialize.py");
    }
    fn creator_fixture(script: &str) {
        let executable=std::env::var("NUS73_BOOTSTRAP_CHECK").expect("checker required");
        let module=std::env::var("NUS73_PROCESS_CHECK_MODULE").unwrap();
        let python=std::env::var("NUS73_PROCESS_CHECK_PYTHON").unwrap();
        let creator=std::env::var("NUS73_BOOTSTRAP_CREATE").unwrap();
        let inventory=serde_json::json!({"bin/s3-local-bootstrap-check":sha256(&std::fs::read(&executable).unwrap()), "bin/s3-local-bootstrap-create":sha256(&std::fs::read(&creator).unwrap())});
        for bps in [0,25] {
            let (mut input,_)=fixture::initial(bps);
            for name in ["chain","exchange","settlement","wallet","sre"] {
                let path=format!("chain/local-demo/components/{name}.json");
                let mut descriptor:serde_json::Value=serde_json::from_slice(&input.files[&path]).unwrap();
                descriptor["implementation_settings"]["artifacts_sha256_json"]=serde_json::json!(
                    String::from_utf8(canonical(&inventory).unwrap()).unwrap());
                input.files.insert(path,canonical(&descriptor).unwrap());
            }
            let contract=fixture::aggregate(&input.files);
            let mut manifest:serde_json::Value=serde_json::from_slice(&input.runtime_manifest).unwrap();
            manifest["contract_sha256"]=serde_json::json!(contract);
            manifest["files_sha256"]=serde_json::json!(input.files.iter().map(|(p,b)|(p.clone(),sha256(b))).collect::<std::collections::BTreeMap<_,_>>());
            input.runtime_manifest=canonical(&manifest).unwrap();
            input.approved_runtime_sha256=sha256(&input.runtime_manifest);
            let mut genesis:serde_json::Value=serde_json::from_slice(&input.genesis).unwrap();
            genesis["app_state"]["contract_hash"]=serde_json::json!(contract);
            input.genesis=canonical(&genesis).unwrap();
            let mut guard:serde_json::Value=serde_json::from_slice(&input.guard).unwrap();
            guard["runtime_manifest_sha256"]=serde_json::json!(input.approved_runtime_sha256);
            guard["context"]["contract_hash"]=serde_json::json!(contract);
            guard["context"]["genesis_hash"]=serde_json::json!(sha256(&input.genesis));
            input.guard=canonical(&guard).unwrap();
            let home=fixture::home(bps);
            let root=home.parent().unwrap().join("checker-integration");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("input.json"),canonical(&fixture::bundle(&input)).unwrap()).unwrap();
            std::fs::write(root.join("profile"),&input.effective_profile).unwrap();
            let output=std::process::Command::new(&python)
                .arg(std::path::Path::new(&module).join(script))
                .arg(&root).arg(&executable).arg(&creator).output().unwrap();
            assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
            let report:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["home_created"],false);
            assert_eq!(report["ready"],true);
            assert!(!home.exists());
            std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
        }
    }
    #[test] fn real_descriptor_capture_fetch_denials_before_rpc() {
        let executable=std::env::var("NUS73_BOOTSTRAP_CHECK").expect("checker required");
        let module=std::env::var("NUS73_PROCESS_CHECK_MODULE").unwrap();
        let python=std::env::var("NUS73_PROCESS_CHECK_PYTHON").unwrap();
        let fetcher=std::env::var("NUS73_BOOTSTRAP_FETCH").unwrap();
        let inventory=serde_json::json!({"bin/s3-local-bootstrap-check":sha256(&std::fs::read(&executable).unwrap()), "bin/s3-local-bootstrap-fetch":sha256(&std::fs::read(&fetcher).unwrap())});
        for bps in [0,25] {
            let (mut input,_)=fixture::initial(bps);
            for name in ["chain","exchange","settlement","wallet","sre"] {
                let path=format!("chain/local-demo/components/{name}.json");
                let mut descriptor:serde_json::Value=serde_json::from_slice(&input.files[&path]).unwrap();
                descriptor["implementation_settings"]["artifacts_sha256_json"]=serde_json::json!(
                    String::from_utf8(canonical(&inventory).unwrap()).unwrap());
                input.files.insert(path,canonical(&descriptor).unwrap());
            }
            let contract=fixture::aggregate(&input.files);
            let mut manifest:serde_json::Value=serde_json::from_slice(&input.runtime_manifest).unwrap();
            manifest["contract_sha256"]=serde_json::json!(contract);
            manifest["files_sha256"]=serde_json::json!(input.files.iter().map(|(p,b)|(p.clone(),sha256(b))).collect::<std::collections::BTreeMap<_,_>>());
            input.runtime_manifest=canonical(&manifest).unwrap();
            input.approved_runtime_sha256=sha256(&input.runtime_manifest);
            let mut genesis:serde_json::Value=serde_json::from_slice(&input.genesis).unwrap();
            genesis["app_state"]["contract_hash"]=serde_json::json!(contract);
            input.genesis=canonical(&genesis).unwrap();
            let mut guard:serde_json::Value=serde_json::from_slice(&input.guard).unwrap();
            guard["runtime_manifest_sha256"]=serde_json::json!(input.approved_runtime_sha256);
            guard["context"]["contract_hash"]=serde_json::json!(contract);
            guard["context"]["genesis_hash"]=serde_json::json!(sha256(&input.genesis));
            input.guard=canonical(&guard).unwrap();
            let home=fixture::home(bps);
            let root=home.parent().unwrap().join("checker-integration");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("input.json"),canonical(&fixture::bundle(&input)).unwrap()).unwrap();
            std::fs::write(root.join("profile"),&input.effective_profile).unwrap();
            let output=std::process::Command::new(&python)
                .arg(std::path::Path::new(&module).join("test_real_bootstrap_fetch.py"))
                .arg(&root).arg(&executable).arg(&fetcher).output().unwrap();
            assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
            let report:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["home_created"],false);
            assert_eq!(report["rpc_attempted"],false);
            assert!(!home.exists());
            std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
        }
    }
    #[test] fn fee_profiles_validate_before_any_home_exists() {
        for fee in [0,25] {
            let (input,_)=fixture::initial(fee); let home=fixture::home(fee);
            let profile=home.parent().unwrap().join("profile.json");
            std::fs::write(&profile,&input.effective_profile).unwrap();
            let raw=canonical(&fixture::bundle(&input)).unwrap();
            let a=args(&profile,&input.approved_runtime_sha256,&sha256(&raw));
            let got=Inputs::parse(a.clone()).unwrap().validate(raw.as_slice()).unwrap();
            assert_eq!(got.context(),Validated::new(input.clone()).unwrap().context());
            let binary=std::env::var("NUS73_BOOTSTRAP_CHECK").expect("compiled check required");
            let capture=home.parent().unwrap().join("capture.json"); std::fs::write(&capture,&raw).unwrap();
            let out=std::process::Command::new(binary).args(a).stdin(std::fs::File::open(capture).unwrap()).output().unwrap();
            assert!(out.status.success()); assert!(out.stderr.is_empty());
            let v:serde_json::Value=serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(v,serde_json::json!({"semantic_validation":true,"approval_verified":false,"service_started":false,"durable_ack":false}));
            assert!(!home.exists());
            std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
        }
    }
    #[test] fn semantic_transport_and_profile_denials() {
        use std::os::unix::fs::symlink;
        let (input,_)=fixture::initial(0); let home=fixture::home(0);
        let profile=home.parent().unwrap().join("profile.json"); std::fs::write(&profile,&input.effective_profile).unwrap();
        let raw=canonical(&fixture::bundle(&input)).unwrap();
        let a=args(&profile,&input.approved_runtime_sha256,&sha256(&raw));
        assert!(Inputs::parse(a.clone()).unwrap().validate(&raw[..raw.len()-1]).is_err());
        let mut bundle=fixture::bundle(&input); bundle["guard"]=serde_json::json!("e30=");
        let bad=canonical(&bundle).unwrap();
        assert!(Inputs::parse(args(&profile,&input.approved_runtime_sha256,&sha256(&bad))).unwrap().validate(bad.as_slice()).is_err());
        std::fs::remove_file(&profile).unwrap(); symlink("missing",&profile).unwrap();
        assert!(Inputs::parse(a).unwrap().validate(raw.as_slice()).is_err());
        assert!(!home.exists()); std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
    }
    #[test] fn invalid_arguments_never_consume_input() {
        let good=args(std::path::Path::new("/profile"),&"a".repeat(64),&"b".repeat(64));
        let mut cases=vec![vec![],good[..good.len()-1].to_vec()];
        for extra in [vec!["--acknowledge-unproven-space"],vec!["--home","/home"],vec!["--runtime-pin","secret"]] {
            let mut a=good.clone(); a.extend(extra.into_iter().map(String::from)); cases.push(a);
        }
        for (i,v) in [(0,"create"),(2,"bad"),(4,"BAD"),(6,"relative")] {
            let mut a=good.clone(); a[i]=v.into(); cases.push(a);
        }
        let binary=std::env::var("NUS73_BOOTSTRAP_CHECK").unwrap();
        for a in cases {
            assert!(Inputs::parse(a.clone()).is_err());
            let mut child=std::process::Command::new(&binary).args(a).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
            while child.try_wait().unwrap().is_none() {
                if std::time::Instant::now()>=deadline {child.kill().unwrap(); child.wait().unwrap(); panic!("invalid CLI waited on stdin");}
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let out=child.wait_with_output().unwrap(); assert_eq!(out.status.code(),Some(2)); assert!(out.stdout.is_empty());
            assert_eq!(out.stderr,b"LOCAL_BOOTSTRAP_CHECK_REJECTED\n");
        }
    }
}

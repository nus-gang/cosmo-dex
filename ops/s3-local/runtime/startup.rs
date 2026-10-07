//! Offline startup assembly. No bind, RPC, tick, home creation or approval claim.
#[path = "driver.rs"] pub mod driver;
#[cfg(feature = "fault-injection")]
#[path = "fault_options.rs"] pub mod fault_options;
#[path = "rest.rs"] pub mod rest;
#[path = "chain_router.rs"] mod chain_router;
pub use chain_router::Helper;
#[path = "signer.rs"] mod signer;
use nus_exchange_contract::{codec, s3::{dev_local::{Engine, Error, Result, Validated},
    journal::sha256, schema, settlement_local::{Options, Rest}}};
#[path = "capture_io.rs"] mod capture_io;
use capture_io::{read_capture, read_regular};
use std::{collections::BTreeMap, fs::OpenOptions, io::Read,
    net::{Ipv4Addr, SocketAddr}, os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf}, sync::Arc, time::Duration};

pub struct Inputs {
    bundle: PathBuf, profile: PathBuf, pin: String, home: PathBuf, keys: PathBuf,
    pub bind: SocketAddr, rpc: SocketAddr, pub lifetime: Duration,
    pub requests: u64, ticks: u64,
}
impl Inputs {
    /// All switches are mandatory, exact and unique. No env/default fallback.
    pub fn parse(args: impl IntoIterator<Item=String>) -> Result<Self> {
        let mut args = args.into_iter();
        let mut flags = BTreeMap::new();
        let mut ack = false;
        while let Some(k) = args.next() {
            if k == "--acknowledge-unproven-space" {
                if ack { return Err("DUPLICATE_OPTION".into()); }
                ack = true; continue;
            }
            if !["--input-set", "--local-demo-profile", "--runtime-pin", "--home",
                "--key-directory", "--bind", "--rpc", "--lifetime-seconds",
                "--max-requests", "--max-ticks"].contains(&k.as_str()) {
                return Err("UNKNOWN_OPTION".into());
            }
            let v = args.next().ok_or(Error::Invalid("OPTION_VALUE"))?;
            if v.is_empty() || v.starts_with("--") { return Err("OPTION_VALUE".into()); }
            if flags.insert(k, v).is_some() { return Err("DUPLICATE_OPTION".into()); }
        }
        if !ack || !flags.contains_key("--local-demo-profile") {
            return Err("LOCAL_DEMO_OPT_IN_REQUIRED".into());
        }
        let get = |k: &str| flags.get(k).map(String::as_str).ok_or(Error::Invalid("OPTION_REQUIRED"));
        let path = |k: &str| -> Result<PathBuf> {
            let p = PathBuf::from(get(k)?);
            if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                return Err("ABSOLUTE_PATH_REQUIRED".into());
            }
            Ok(p)
        };
        let number = |k: &str, max: u64| -> Result<u64> {
            let raw = get(k)?;
            let n = raw.parse::<u64>().map_err(|_| Error::Invalid("RESOURCE_LIMIT"))?;
            if n == 0 || n > max || n.to_string() != raw { return Err("RESOURCE_LIMIT".into()); }
            Ok(n)
        };
        let endpoint = |k: &str| -> Result<SocketAddr> {
            let raw = get(k)?;
            let addr: SocketAddr = raw.parse().map_err(|_| Error::Invalid("ENDPOINT"))?;
            if addr.ip() != Ipv4Addr::LOCALHOST || addr.port() < 1024 || addr.to_string() != raw {
                return Err("ENDPOINT".into());
            }
            Ok(addr)
        };
        let pin = get("--runtime-pin")?;
        if pin.len()!=64 || !pin.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err("RUNTIME_PIN".into());
        }
        let bind = endpoint("--bind")?; let rpc = endpoint("--rpc")?;
        if bind == rpc { return Err("ENDPOINT_COLLISION".into()); }
        Ok(Self { bundle:path("--input-set")?, profile:path("--local-demo-profile")?,
            pin:pin.into(), home:path("--home")?, keys:path("--key-directory")?, bind, rpc,
            lifetime:Duration::from_secs(number("--lifetime-seconds",3600)?),
            requests:number("--max-requests",100_000)?, ticks:number("--max-ticks",3600)? })
    }
    /// Decode approved component binding; opens an EXISTING home only. This
    /// checks bytes, not independent approval or on-disk executable hashes.
    /// The future launcher must pass those gates before calling this function.
    pub fn prepare(self) -> Result<Prepared> {
        let raw = read_regular(&self.bundle,48*1024*1024)?;
        self.prepare_bytes(raw)
    }
    /// Consume preflight's captured bytes instead of reopening --input-set.
    /// The hash binds this transport only: it is NOT an organizational approval.
    /// The caller supplies a finite, already-collected pipe or memory reader.
    pub fn prepare_captured(self, reader: impl Read, capture_sha256: &str) -> Result<Prepared> {
        let raw = read_capture(reader, capture_sha256)?;
        self.prepare_bytes(raw)
    }
    fn prepare_bytes(self, raw: Vec<u8>) -> Result<Prepared> {
        let validated = Validated::decode_bundle(&raw, read_regular(&self.profile,1024*1024)?, self.pin, true)?;
        // Read genesis from the exact already-validated bundle bytes.
        let bundle = codec::unique_json(&raw).map_err(|_| Error::Invalid("BUNDLE"))?;
        let genesis = codec::unique_json(&schema::bytes(&bundle["genesis"])?).map_err(|_| Error::Invalid("GENESIS"))?;
        let engine = Arc::new(Engine::open(&self.home,validated)?);
        let view = engine.reader().get()?;
        let anchors = engine.trusted_recovery_history(&view.commit,None,1)?;
        let owner = schema::bytes(&anchors.latest.snapshot.value()["operator"])?;
        let mut matched = None;
        for key in genesis["app_state"]["settlement_operator_public_keys"].as_array().ok_or(Error::Invalid("GENESIS_KEYS"))? {
            let pk = schema::bytes(key)?;
            if hex::decode(&sha256(&pk)[..40]).map_err(|_| Error::Invalid("OPERATOR"))? == owner {
                if matched.replace(pk).is_some() { return Err("OPERATOR_AMBIGUOUS".into()); }
            }
        }
        let signer = signer::LocalSigner::load(&self.keys,&matched.ok_or(Error::Invalid("OPERATOR_NOT_IN_GENESIS"))?)?;
        let rest = Rest::new(engine.clone(),anchors.applied.snapshot,Options {
            enabled:true, acknowledge_unproven_space:true, bind:self.bind.ip() })?;
        let driver = driver::Driver::recover(engine,self.rpc,signer,self.lifetime,self.ticks)?;
        Ok(Prepared { rest, driver, chain:chain_router::ChainRead::new(self.rpc)?, bind:self.bind,
            limits:rest::lifecycle::Limits { lifetime:self.lifetime, requests:self.requests } })
    }
}
pub struct Prepared {
    chain: chain_router::ChainRead,
    pub rest: Rest, pub driver: driver::Driver<signer::LocalSigner>,
    pub bind: SocketAddr, pub limits: rest::lifecycle::Limits,
}
impl Prepared {
    /// Consume process resources; approval and exact-byte gates must already
    /// have passed before the caller creates this listener. This never binds.
    pub fn serve(self, listener: std::net::TcpListener, stop: &std::sync::atomic::AtomicBool, helper: &Helper)
        -> std::result::Result<rest::lifecycle::Report, &'static str> {
        self.run_with(|p| {
            let observed = std::cell::RefCell::new(None);
            rest::lifecycle::serve(listener,p.bind,stop,p.limits,
                || {
                    *observed.borrow_mut() = Some(p.driver.tick_snapshot().map_err(|_| "WORKER_CLOSED")?);
                    Ok(())
                },
                |stream| {
                    let pair = observed.borrow();
                    let (anchor, observation) = pair.as_ref().ok_or("OBSERVATION_UNAVAILABLE")?;
                    chain_router::serve(stream,&p.rest,observation,anchor,&p.chain,helper,stop,|| {
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()
                            .and_then(|d|u64::try_from(d.as_millis()).ok())
                            .ok_or(Error::Invalid("CLOCK_UNAVAILABLE"))
                    })
                })
        })
    }
    fn run_with(mut self, run: impl FnOnce(&mut Self) -> std::result::Result<rest::lifecycle::Report, &'static str>)
        -> std::result::Result<rest::lifecycle::Report, &'static str> {
        let result=run(&mut self);
        self.driver.stop();
        // self owns every Engine Arc and the private signer. Returning or
        // unwinding drops them; no mutable home file is deleted or repaired.
        result
    }
}
#[cfg(test)]
#[path="../../../exchange/tests/support/dev_fixture.rs"] mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _,engine::general_purpose::STANDARD};
    use std::os::unix::fs::{PermissionsExt,symlink};
    fn args() -> Vec<String> {
        ["--input-set","/bundle","--local-demo-profile","/profile","--runtime-pin",&"a".repeat(64),
            "--home","/home","--key-directory","/keys","--bind","127.0.0.1:18080","--rpc","127.0.0.1:26657",
            "--lifetime-seconds","60","--max-requests","100","--max-ticks","60","--acknowledge-unproven-space"]
            .into_iter().map(String::from).collect()
    }
    #[test] fn offline_descriptor_capture_real_validator() {
        use nus_exchange_contract::s3::journal::canonical;
        let executable=std::env::var("NUS73_PREFLIGHT_EXECUTABLE").expect("real validator required");
        let python=std::env::var("NUS73_PROCESS_CHECK_PYTHON").expect("python required");
        let module=std::env::var("NUS73_PROCESS_CHECK_MODULE").expect("module required");
        let binary=std::fs::read(&executable).unwrap();
        let worker=std::env::var("NUS73_READY_WORKER_EXECUTABLE").ok();
        let mut inventory=serde_json::json!({"bin/s3-local-preflight":sha256(&binary)});
        if let Some(path)=&worker { inventory["bin/s3-local-worker"]=serde_json::json!(sha256(&std::fs::read(path).unwrap())); }
        if worker.is_some() {
            let helper=std::env::var("NUS73_DIRECT_HELPER_EXECUTABLE").expect("direct helper required with worker");
            inventory["bin/nus-s3-local-direct"]=serde_json::json!(sha256(&std::fs::read(helper).unwrap()));
        }
        if let Ok(path)=std::env::var("NUS73_STORAGE_FAULT_EXECUTABLE") {
            inventory["bin/s3-local-storage-fault"]=serde_json::json!(sha256(&std::fs::read(path).unwrap()));
        }
        inventory["web/index.html"]=serde_json::json!(sha256(b"<!doctype html><title>synthetic web fixture</title>"));
        inventory["web/page.js"]=serde_json::json!(sha256(b"// synthetic web fixture"));
        for bps in [0,25] {
            let (mut input,mut initial)=fixture::initial(bps);
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
            manifest["files_sha256"]=serde_json::json!(input.files.iter().map(|(p,b)|(p.clone(),sha256(b))).collect::<BTreeMap<_,_>>());
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
            let validated=Validated::new(input.clone()).unwrap();
            initial["context"]=validated.context().clone(); fixture::finish(&mut initial);
            let home=fixture::home(bps);
            let engine=Engine::create(&home,validated,&canonical(&initial).unwrap()).unwrap();
            let commit=engine.reader().get().unwrap().commit.clone(); drop(engine);
            let public=home.parent().unwrap().join(format!("offline-check-{bps}"));
            std::fs::create_dir(&public).unwrap();
            std::fs::write(public.join("input.json"),canonical(&fixture::bundle(&input)).unwrap()).unwrap();
            std::fs::write(public.join("profile"),&input.effective_profile).unwrap();
            let keys=public.join("keys"); std::fs::create_dir(&keys).unwrap();
            std::fs::set_permissions(&keys,std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(keys.join("operator.seed"),hex::decode(fixture::key(16)["test_seed_hex"].as_str().unwrap()).unwrap()).unwrap();
            std::fs::set_permissions(keys.join("operator.seed"),std::fs::Permissions::from_mode(0o600)).unwrap();
            let mut a=args();
            for (i,p) in [(1,public.join("input.json")),(3,public.join("profile")),(7,home.clone()),(9,keys)] { a[i]=p.to_str().unwrap().into(); }
            a[5]=input.approved_runtime_sha256.clone();
            let result=std::process::Command::new(&python).arg(Path::new(&module).join("test_real_validator.py"))
                .arg(&public).arg(&executable).args(&a).output().unwrap();
            assert!(result.status.success(),"offline check failed: {}",String::from_utf8_lossy(&result.stderr));
            let report:serde_json::Value=serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(report["semantic_preflight"]["semantic_validation"],true);
            assert_eq!(report["approval_verified"],false);
            assert_eq!(report["services_started"],false);
            for _ in 0..2 {
                let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
            }
            std::fs::remove_dir_all(public).unwrap(); std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[test] fn captured_input_rejects_changed_truncated_oversize_and_io() {
        let raw=b"captured input";
        let hash=sha256(raw);
        assert_eq!(read_capture(&raw[..],&hash).unwrap(),raw);
        for bytes in [&b""[..],&raw[..raw.len()-1],&b"changed input"[..]] {
            assert!(read_capture(bytes,&hash).is_err());
        }
        assert!(read_capture(&raw[..],"bad").is_err());
        assert!(read_capture(std::io::repeat(0),&hash).is_err());
        struct Broken;
        impl Read for Broken { fn read(&mut self,_:&mut [u8])->std::io::Result<usize> {
            Err(std::io::Error::other("injected capture IO"))
        }}
        assert!(read_capture(Broken,&hash).is_err());
    }
    #[test] fn startup_rejects_missing_duplicate_unsafe_and_unbounded_inputs() {
        assert!(Inputs::parse(args()).is_ok());
        for i in [0,2,4,6,8,10,12,14,16,18] {
            let mut a=args(); a.drain(i..i+2); assert!(Inputs::parse(a).is_err());
        }
        let mut a=args(); a.pop(); assert!(Inputs::parse(a).is_err());
        for (i,value) in [(11,"0.0.0.0:18080"),(11,"[::1]:18080"),(11,"localhost:18080"),
            (11,"127.0.0.1:80"),(13,"127.0.0.1:18080"),(15,"0"),(15,"3601"),(17,"100001"),
            (19,"3601"),(15,"+1"),(15,"01"),(5,"test-pin"),(7,"relative"),(1,"/a/../bundle")] {
            let mut a=args(); a[i]=value.into(); assert!(Inputs::parse(a).is_err(),"{value}");
        }
        for extra in [vec!["--unknown"],vec!["--acknowledge-unproven-space"],vec!["--max-ticks","2"]] {
            let mut a=args(); a.extend(extra.into_iter().map(String::from)); assert!(Inputs::parse(a).is_err());
        }
    }
    #[test] fn startup_regular_file_refuses_links_fifo_directory_and_oversize() {
        let root=fixture::home(0); std::fs::create_dir(&root).unwrap();
        let p=root.join("input"); std::fs::write(&p,b"abc").unwrap();
        assert_eq!(read_regular(&p,3).unwrap(),b"abc"); assert!(read_regular(&p,2).is_err());
        symlink(&p,root.join("link")).unwrap(); assert!(read_regular(&root.join("link"),3).is_err());
        std::fs::hard_link(&p,root.join("hard")).unwrap(); assert!(read_regular(&p,3).is_err());
        assert!(read_regular(&root,4096).is_err());
        let fifo=root.join("fifo"); let c=std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe{libc::mkfifo(c.as_ptr(),0o600)},0); assert!(read_regular(&fifo,3).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn startup_prepare_opens_existing_home_and_releases_writer_on_drop_or_error() {
        for bps in [0,25] {
            let (input,initial)=fixture::initial(bps); let home=fixture::home(bps);
            let engine=Engine::create(&home,Validated::new(input.clone()).unwrap(),&nus_exchange_contract::s3::journal::canonical(&initial).unwrap()).unwrap();
            let commit=engine.reader().get().unwrap().commit.clone(); drop(engine);
            let public=home.parent().unwrap().join(format!("startup-input-{bps}")); std::fs::create_dir(&public).unwrap();
            let bundle=serde_json::json!({"runtime_manifest":STANDARD.encode(&input.runtime_manifest),"files":input.files.iter().map(|(k,v)|(k.clone(),STANDARD.encode(v))).collect::<BTreeMap<_,_>>(),"guard":STANDARD.encode(&input.guard),"genesis":STANDARD.encode(&input.genesis)});
            std::fs::write(public.join("bundle"),serde_json::to_vec(&bundle).unwrap()).unwrap();
            std::fs::write(public.join("profile"),&input.effective_profile).unwrap();
            let keys=public.join("keys"); std::fs::create_dir(&keys).unwrap(); std::fs::set_permissions(&keys,std::fs::Permissions::from_mode(0o700)).unwrap();
            let seed=hex::decode(fixture::key(16)["test_seed_hex"].as_str().unwrap()).unwrap();
            std::fs::write(keys.join("operator.seed"),seed).unwrap(); std::fs::set_permissions(keys.join("operator.seed"),std::fs::Permissions::from_mode(0o600)).unwrap();
            let mut a=args();
            for (i,p) in [(1,public.join("bundle")),(3,public.join("profile")),(7,home.clone()),(9,keys.clone())] { a[i]=p.to_str().unwrap().into(); }
            a[5]=input.approved_runtime_sha256.clone();
            for _ in 0..2 {
                let prepared=Inputs::parse(a.clone()).unwrap().prepare().unwrap();
                assert!(Engine::open(&home,Validated::new(input.clone()).unwrap()).is_err());
                drop(prepared);
                let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap(); assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
            }
            // Optional process harness supplied by the recorded executable build.
            // Its stdin is a finite regular fixture file; no service is started.
            if let Ok(executable)=std::env::var("NUS73_PREFLIGHT_EXECUTABLE") {
                for _ in 0..2 {
                    let raw=std::fs::read(public.join("bundle")).unwrap();
                    let result=std::process::Command::new(&executable)
                        .args(["validate-captured","--capture-sha256",&sha256(&raw)])
                        .args(&a).stdin(std::fs::File::open(public.join("bundle")).unwrap())
                        .output().unwrap();
                    assert!(result.status.success());
                    assert!(result.stderr.is_empty());
                    assert_eq!(serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                        serde_json::json!({"semantic_validation":true,"approval_verified":false,
                            "service_started":false,"durable_ack":false}));
                    let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap();
                    assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
                }
            }
            // Exercise the bounded Python parent with the real Rust validator.
            // Test-only transport: descriptor/approval validation remains separate.
            if let (Ok(executable),Ok(python),Ok(module))=(
                std::env::var("NUS73_PREFLIGHT_EXECUTABLE"),
                std::env::var("NUS73_PROCESS_CHECK_PYTHON"),
                std::env::var("NUS73_PROCESS_CHECK_MODULE")) {
                let code="import sys,json; sys.path.insert(0,sys.argv[1]); from process_check import validate_captured; raw=open(sys.argv[3],'rb').read(); print(json.dumps(validate_captured(sys.argv[2],sys.argv[4:],raw)))";
                let result=std::process::Command::new(python).args(["-c",code,&module,&executable])
                    .arg(public.join("bundle")).args(&a).output().unwrap();
                assert!(result.status.success(),"bounded supervisor failed: {:?}",result.status);
                assert!(result.stderr.is_empty());
                assert_eq!(serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                    serde_json::json!({"semantic_validation":true,"approval_verified":false,
                        "service_started":false,"durable_ack":false}));
                let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
            }
            // Real service binary prepares, then receives denial only. Never START.
            if let Ok(executable)=std::env::var("NUS73_WORKER_EXECUTABLE") {
                let python=std::env::var("NUS73_PROCESS_CHECK_PYTHON").unwrap();
                let module=std::env::var("NUS73_PROCESS_CHECK_MODULE").unwrap();
                let result=std::process::Command::new(python)
                    .arg(Path::new(&module).join("test_worker_denial.py"))
                    .arg(executable).arg(public.join("bundle")).args(&a).output().unwrap();
                assert!(result.status.success(),"worker denial failed: {}",String::from_utf8_lossy(&result.stderr));
                assert_eq!(result.stdout,b"WORKER_DENIAL_PASS\n");
                for _ in 0..2 {
                    let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap();
                    assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
                }
            }
            // A replacement at the old input path cannot replace captured bytes.
            let captured=std::fs::read(public.join("bundle")).unwrap();
            std::fs::write(public.join("bundle"),b"REPLACED_AFTER_PREFLIGHT").unwrap();
            assert!(Inputs::parse(a.clone()).unwrap().prepare().is_err());
            let prepared=Inputs::parse(a.clone()).unwrap()
                .prepare_captured(&captured[..],&sha256(&captured)).unwrap();
            drop(prepared);
            assert!(Inputs::parse(a.clone()).unwrap()
                .prepare_captured(&b"CHANGED"[..],&sha256(&captured)).is_err());
            let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap();
            assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
            std::fs::write(public.join("bundle"),&captured).unwrap();
            for mode in 0..3 {
                let prepared=Inputs::parse(a.clone()).unwrap().prepare().unwrap();
                let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepared.run_with(|_| {
                    match mode {
                        0 => Ok(rest::lifecycle::Report { reason:rest::lifecycle::Stop::Requested,handled:0,rejected:0,ticks:0 }),
                        1 => Err("INJECTED_SERVICE_ERROR"),
                        _ => panic!("injected service unwind"),
                    }
                })));
                match mode { 0 => assert!(result.unwrap().is_ok()),1 => assert_eq!(result.unwrap(),Err("INJECTED_SERVICE_ERROR")),_ => assert!(result.is_err()) }
                let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
            }
            std::fs::write(keys.join("operator.seed"),[0;32]).unwrap();
            assert!(Inputs::parse(a.clone()).unwrap().prepare().is_err());
            let e=Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap(); assert_eq!(e.reader().get().unwrap().commit,commit); drop(e);
            a[7]=public.join("missing-home").to_str().unwrap().into();
            assert!(Inputs::parse(a).unwrap().prepare().is_err()); assert!(!public.join("missing-home").exists());
            std::fs::remove_dir_all(public).unwrap(); std::fs::remove_dir_all(home).unwrap();
        }
    }
}

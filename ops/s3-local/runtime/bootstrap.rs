//! First trusted RPC snapshot -> approved C create. No genesis-derived ledger.
//! Caller must audit runtime approval before fetch and preserve returned RPC
//! bytes as evidence before create. No retry, repair, service or signer here.
#[path = "query.rs"]
mod query;
use nus_exchange_contract::{codec, s3::{
    dev_local::{Engine, Error, Result, Validated},
    evidence::{self, RPC, TYPED}, journal::canonical, schema,
}};
use serde_json::json;
use std::{net::SocketAddr, path::Path};

/// One bounded trusted loopback query. Raw response remains caller-owned even
/// when later semantic validation or home publication fails. L-T only.
pub fn fetch(config: &Validated, addr: SocketAddr) -> Result<Vec<u8>> {
    let rpc = query::QueryRpc::new(addr)?;
    let data = hex::encode(canonical(&json!({"context":config.context(),"height":"0"}))?);
    Ok(rpc.fetch(query::Query::Abci {
        path: "/nus.exchange.s3.v1.Query/Snapshot", data_hex: &data, height: 0,
    })?)
}

/// Transport checks precede all home writes. C retains authority over registered
/// owners, conservation, fresh bootstrap and its no-replace/fsync/writer lock.
/// Caller keeps raw_rpc; exact decoded snapshot bytes are persisted by C.
pub fn create(home: &Path, config: Validated, raw_rpc: &[u8]) -> Result<Engine> {
    let reference = evidence::reference(raw_rpc, RPC)?;
    evidence::verify(&reference, raw_rpc)?;
    let v = codec::unique_json(raw_rpc)?;
    if v["jsonrpc"] != "2.0" || v["id"] != 1 || v.get("error").is_some() {
        return Err(Error::Invalid("RPC_ERROR"));
    }
    let r = &v["result"]["response"];
    if r["code"] != 0 && r["code"] != "0" {
        return Err(Error::Invalid("RPC_ERROR"));
    }
    let raw = schema::bytes(&r["value"])?;
    evidence::reference(&raw, TYPED)?;
    let snapshot = schema::decode("ChainSnapshot", &raw)?;
    if snapshot["context"] != *config.context()
        || schema::num(&snapshot["height"])? == 0
        || r["height"] != snapshot["height"]
        || canonical(&snapshot)? != raw {
        return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
    }
    Engine::create(home, config, &raw)
}

/// Preserve bounded raw RPC before semantic validation/store creation. The
/// evidence directory must already be private and distinct from the new home.
/// Partial evidence is retained on failure; never replace or retry in place.
pub fn create_preserved(home: &Path, config: Validated, raw_rpc: &[u8], directory: &Path) -> Result<Engine> {
    preserve_rpc(directory, raw_rpc)?;
    create(home, config, raw_rpc)
}

/// Internal one-shot orchestration. `audit` must freshly check the exact runtime
/// candidate on each call; this callback is not an approval implementation or
/// reusable permit. Fetch is L-T-only. Received bytes survive later revocation.
pub fn initialize_with(
    home: &Path, config: Validated, directory: &Path,
    mut audit: impl FnMut(&Validated) -> Result<()>,
    fetch_once: impl FnOnce(&Validated) -> Result<Vec<u8>>,
    stopped: impl Fn() -> bool,
) -> Result<Engine> {
    let check_stop = || if stopped() { Err(Error::Invalid("BOOTSTRAP_STOPPED")) } else { Ok(()) };
    check_stop()?;
    audit(&config)?;
    check_stop()?;
    let raw = fetch_once(&config)?;
    // Preserve even if cancellation/revocation happened while fetching.
    preserve_rpc(directory, &raw)?;
    check_stop()?;
    audit(&config)?;
    check_stop()?;
    create(home, config, &raw)
}

pub(crate) fn preserve_rpc(directory: &Path, raw: &[u8]) -> Result<()> {
    preserve_rpc_with_sync(directory, raw, |file, _| file.sync_all())
}

// Internal IO seam; production always uses File::sync_all. No runtime fault flag.
fn preserve_rpc_with_sync(
    directory: &Path, raw: &[u8],
    mut sync: impl FnMut(&std::fs::File, bool) -> std::io::Result<()>,
) -> Result<()> {
    use std::{fs::{File, OpenOptions}, io::Write,
        os::{fd::{AsRawFd, FromRawFd}, unix::fs::{MetadataExt, OpenOptionsExt}}};
    evidence::reference(raw, RPC)?;
    if !directory.is_absolute() || directory.canonicalize()? != directory {
        return Err(Error::Invalid("EVIDENCE_PATH"));
    }
    let root = OpenOptions::new().read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(directory)?;
    let meta = root.metadata()?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o777 != 0o700 {
        return Err(Error::Invalid("EVIDENCE_PRIVATE_ROOT"));
    }
    let fd = unsafe { libc::openat(root.as_raw_fd(), c"bootstrap-rpc.json".as_ptr(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(std::io::Error::last_os_error().into()); }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(raw)?;
    sync(&file, false)?;
    sync(&root, true)?;
    let current = std::fs::symlink_metadata(directory)?;
    if current.dev() != meta.dev() || current.ino() != meta.ino() {
        return Err(Error::Invalid("EVIDENCE_ROOT_CHANGED"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../exchange/tests/support/dev_fixture.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde_json::Value;
    fn rpc(v: &Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":{"response":{
            "code":0,"height":v["height"],"value":STANDARD.encode(canonical(v).unwrap())}}})).unwrap()
    }
    fn cleanup(home: &Path) { std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap(); }
    #[test]
    fn evidence_sync_failures_preserve_bytes_and_prevent_create() {
        use std::os::unix::fs::PermissionsExt;
        for bps in [0,25] {
            for fail_root in [false,true] {
                let (input,v)=fixture::initial(bps); let home=fixture::home(bps);
                let dir=home.parent().unwrap().join("sync-failure");
                std::fs::create_dir(&dir).unwrap();
                std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o700)).unwrap();
                let raw=rpc(&v); let mut calls=Vec::new();
                let result=preserve_rpc_with_sync(&dir,&raw,|file,is_root| {
                    calls.push(is_root);
                    if is_root==fail_root {return Err(std::io::Error::other("injected sync failure"));}
                    file.sync_all()
                }).and_then(|()|create(&home,Validated::new(input).unwrap(),&raw));
                assert!(result.is_err()); assert!(!home.exists());
                assert_eq!(calls,if fail_root {vec![false,true]} else {vec![false]});
                assert_eq!(std::fs::read(dir.join("bootstrap-rpc.json")).unwrap(),raw);
                assert!(preserve_rpc(&dir,&raw).is_err());
                cleanup(&home);
            }
        }
    }
    #[test]
    fn evidence_root_replacement_is_detected_after_sync() {
        use std::os::unix::fs::PermissionsExt;
        let (input,v)=fixture::initial(0); let home=fixture::home(0);
        let dir=home.parent().unwrap().join("evidence");
        let saved=home.parent().unwrap().join("evidence-preserved");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o700)).unwrap();
        let raw=rpc(&v);
        let result=preserve_rpc_with_sync(&dir,&raw,|file,is_root| {
            file.sync_all()?;
            if is_root {
                std::fs::rename(&dir,&saved)?;
                std::fs::create_dir(&dir)?;
                std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        }).and_then(|()|create(&home,Validated::new(input).unwrap(),&raw));
        assert!(matches!(result,Err(Error::Invalid("EVIDENCE_ROOT_CHANGED"))));
        assert!(!home.exists()); assert!(!dir.join("bootstrap-rpc.json").exists());
        assert_eq!(std::fs::read(saved.join("bootstrap-rpc.json")).unwrap(),raw);
        cleanup(&home);
    }
    #[test]
    fn initialization_orders_fresh_audits_and_preserves_before_second() {
        use std::{cell::RefCell, os::unix::fs::PermissionsExt};
        for bps in [0,25] {
            let (input,v)=fixture::initial(bps); let home=fixture::home(bps);
            let dir=home.parent().unwrap().join("init-evidence");
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o700)).unwrap();
            let events=RefCell::new(Vec::new()); let raw=rpc(&v);
            let config=Validated::new(input).unwrap();
            let e=initialize_with(&home,config.clone(),&dir,|c| {
                assert_eq!(c.context(),config.context());
                let mut events=events.borrow_mut();
                if !events.is_empty() { assert_eq!(std::fs::read(dir.join("bootstrap-rpc.json")).unwrap(),raw); }
                events.push("audit"); Ok(())
            },|_| { events.borrow_mut().push("fetch"); Ok(raw.clone()) },||false).unwrap();
            assert_eq!(*events.borrow(),["audit","fetch","audit"]);
            let before=e.reader().get().unwrap(); drop(e);
            for _ in 0..2 { let e=Engine::open(&home,config.clone()).unwrap();
                let after=e.reader().get().unwrap(); assert_eq!(before.commit,after.commit); assert_eq!(before.state,after.state); }
            cleanup(&home);
        }
    }
    #[test]
    fn initialization_denials_never_create_or_retry() {
        use std::{cell::Cell, os::unix::fs::PermissionsExt};
        for case in ["initial-stop","first-audit","audit-stop","fetch-error","fetch-stop","second-audit","final-stop","collision"] {
            let (input,v)=fixture::initial(0); let home=fixture::home(0);
            let dir=home.parent().unwrap().join("init-evidence");
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o700)).unwrap();
            let path=dir.join("bootstrap-rpc.json");
            if case=="collision" {std::fs::write(&path,b"existing").unwrap();}
            let audits=Cell::new(0); let fetches=Cell::new(0); let stop=Cell::new(case=="initial-stop"); let raw=rpc(&v);
            let result=initialize_with(&home,Validated::new(input).unwrap(),&dir,|_| {
                audits.set(audits.get()+1);
                if (case=="first-audit" && audits.get()==1)||(case=="second-audit" && audits.get()==2) {return Err(Error::Invalid("AUDIT_DENIED"));}
                if case=="audit-stop" || (case=="final-stop" && audits.get()==2) {stop.set(true);}
                Ok(())
            },|_| {fetches.set(fetches.get()+1); if case=="fetch-error" {return Err(Error::Invalid("FETCH_ERROR"));}
                if case=="fetch-stop" {stop.set(true);} Ok(raw.clone())},||stop.get());
            assert!(result.is_err()); assert!(!home.exists()); assert!(fetches.get()<=1);
            let fetched=!["initial-stop","first-audit","audit-stop"].contains(&case);
            assert_eq!(fetches.get(),u32::from(fetched));
            if case=="collision" {assert_eq!(std::fs::read(path).unwrap(),b"existing");}
            else if fetched && case!="fetch-error" {assert_eq!(std::fs::read(path).unwrap(),raw);}
            else {assert!(!path.exists());}
            cleanup(&home);
        }
    }
    #[test]
    fn preserved_rpc_precedes_create_and_survives_semantic_failure() {
        use std::os::unix::fs::{PermissionsExt, MetadataExt};
        for bps in [0,25] {
            for valid in [true,false] {
                let (input,v)=fixture::initial(bps); let home=fixture::home(bps);
                let dir=home.parent().unwrap().join("rpc-evidence");
                std::fs::create_dir(&dir).unwrap();
                std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o700)).unwrap();
                let raw=if valid {rpc(&v)} else {b"invalid rpc".to_vec()};
                let c=Validated::new(input).unwrap();
                let result=create_preserved(&home,c.clone(),&raw,&dir);
                assert_eq!(result.is_ok(),valid); drop(result);
                let path=dir.join("bootstrap-rpc.json");
                assert_eq!(std::fs::read(&path).unwrap(),raw);
                assert_eq!(path.metadata().unwrap().mode() & 0o777,0o600);
                assert_eq!(home.exists(),valid);
                assert!(create_preserved(&home,c.clone(),&raw,&dir).is_err());
                if valid { for _ in 0..2 { drop(Engine::open(&home,c.clone()).unwrap()); } }
                cleanup(&home);
            }
        }
    }
    #[test]
    fn evidence_collision_permissions_links_and_cap_prevent_home() {
        use std::os::unix::fs::{PermissionsExt,symlink};
        for case in ["collision","link","public","oversize"] {
            let (input,v)=fixture::initial(0); let home=fixture::home(0);
            let dir=home.parent().unwrap().join("rpc-evidence");
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o700)).unwrap();
            let path=dir.join("bootstrap-rpc.json");
            match case {
                "collision"=>std::fs::write(&path,b"preserve").unwrap(),
                "link"=>symlink("missing",&path).unwrap(),
                "public"=>std::fs::set_permissions(&dir,std::fs::Permissions::from_mode(0o755)).unwrap(),
                _=>(),
            }
            let raw=if case=="oversize" {vec![b' ';262144*65]} else {rpc(&v)};
            assert!(create_preserved(&home,Validated::new(input).unwrap(),&raw,&dir).is_err());
            assert!(!home.exists());
            if case=="collision" {assert_eq!(std::fs::read(&path).unwrap(),b"preserve");}
            if case=="link" {assert!(path.is_symlink());}
            cleanup(&home);
        }
    }
    #[test]
    fn bootstrap_create_replay_exact_bytes_and_writer() {
        for bps in [0,25] {
            let (input,v)=fixture::initial(bps);
            let home=fixture::home(bps);
            let raw=rpc(&v); let original=raw.clone();
            let c=Validated::new(input).unwrap();
            let e=create(&home,c.clone(),&raw).unwrap();
            let before=e.reader().get().unwrap();
            assert_eq!(before.state["chain_snapshot"],v);
            assert_eq!(std::fs::read(home.join("bootstrap.dev.json")).unwrap(),canonical(&v).unwrap());
            assert!(create(&home,c.clone(),&raw).is_err());
            assert!(Engine::open(&home,c.clone()).is_err());
            drop(e);
            for _ in 0..2 {
                let e=Engine::open(&home,c.clone()).unwrap();
                let after=e.reader().get().unwrap();
                assert_eq!(before.commit,after.commit); assert_eq!(before.state,after.state);
            }
            assert_eq!(raw,original); cleanup(&home);
        }
    }
    #[test]
    fn malformed_transport_creates_no_home() {
        let (input,v)=fixture::initial(0); let c=Validated::new(input).unwrap();
        let good:Value=serde_json::from_slice(&rpc(&v)).unwrap();
        let mut cases=vec![];
        for (pointer,value) in [("/id",json!(2)),("/jsonrpc",json!("1.0")),
            ("/result/response/code",json!(1)),("/result/response/height",json!("101")),
            ("/result/response/value",json!("!"))] {
            let mut bad=good.clone(); *bad.pointer_mut(pointer).unwrap()=value;
            cases.push(serde_json::to_vec(&bad).unwrap());
        }
        let mut error=good.clone(); error["error"]=json!(null); cases.push(serde_json::to_vec(&error).unwrap());
        let mut noncanonical=good.clone();
        noncanonical["result"]["response"]["value"]=json!(STANDARD.encode(serde_json::to_vec_pretty(&v).unwrap()));
        cases.push(serde_json::to_vec(&noncanonical).unwrap());
        cases.push(b"{\"id\":1,\"id\":1}".to_vec());
        cases.push(vec![b' ';262144*65]);
        for raw in cases {
            let home=fixture::home(0);
            assert!(create(&home,c.clone(),&raw).is_err()); assert!(!home.exists()); cleanup(&home);
        }
    }
    #[test]
    fn c_rejects_invalid_bootstrap_before_publication() {
        for bps in [0,25] {
            let (input,v)=fixture::initial(bps); let c=Validated::new(input).unwrap();
            for pointer in ["/context/genesis_hash","/snapshot_id","/accounts/0/assets/0/confirmed_atoms","/terminal_batch_seqs"] {
                let mut bad=v.clone();
                *bad.pointer_mut(pointer).unwrap()=match pointer {
                    "/accounts/0/assets/0/confirmed_atoms"=>json!("999999999999999"),
                    "/terminal_batch_seqs"=>json!(["1"]), _=>json!("ff".repeat(32)),
                };
                if pointer!="/snapshot_id" { fixture::finish(&mut bad); }
                let home=fixture::home(bps);
                assert!(create(&home,c.clone(),&rpc(&bad)).is_err());
                assert!(!home.exists()); cleanup(&home);
            }
        }
    }
}

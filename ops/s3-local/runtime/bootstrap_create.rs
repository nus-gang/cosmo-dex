//! Internal offline create child. Parent owns authenticated approval and RPC
//! acquisition. READY/START is ordering only, never organizational approval.
#[path="bootstrap.rs"] mod bootstrap;
#[path="bootstrap_check.rs"] mod check;
#[path="capture_io.rs"] mod capture_io;
#[path="start_gate.rs"] mod start_gate;
#[path="signals.rs"] mod signals;
use nus_exchange_contract::s3::{dev_local::{Error, Result, Validated}, evidence::RPC, journal::sha256};
use std::{path::PathBuf, io::Write, os::{fd::FromRawFd, unix::net::UnixStream}, sync::atomic::Ordering, time::Duration};

struct Inputs { fd:i32, home:PathBuf, evidence:PathBuf, rpc:PathBuf, rpc_hash:String, check:check::Inputs }
impl Inputs {
    fn parse(args:impl IntoIterator<Item=String>)->Result<Self> {
        let mut a=args.into_iter();
        if a.next().as_deref()!=Some("create-captured") { return Err(Error::Invalid("MODE")); }
        let mut value=|name:&str|->Result<String> {
            if a.next().as_deref()!=Some(name) { return Err(Error::Invalid("OPTION")); }
            a.next().ok_or(Error::Invalid("OPTION_VALUE"))
        };
        let raw_fd=value("--start-gate-fd")?;
        let fd=raw_fd.parse::<i32>().map_err(|_|Error::Invalid("FD"))?;
        if !(3..=1024).contains(&fd) || fd.to_string()!=raw_fd { return Err(Error::Invalid("FD")); }
        let home=PathBuf::from(value("--home")?);
        let evidence=PathBuf::from(value("--evidence-root")?);
        let rpc=PathBuf::from(value("--rpc-file")?);
        let rpc_hash=value("--rpc-sha256")?;
        for p in [&home,&evidence,&rpc] {
            if !p.is_absolute() || p.components().any(|c|matches!(c,std::path::Component::ParentDir)) {
                return Err(Error::Invalid("INPUT_PATH"));
            }
        }
        if rpc_hash.len()!=64 || !rpc_hash.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)) {
            return Err(Error::Invalid("INPUT_HASH"));
        }
        let check=check::Inputs::parse(std::iter::once("validate-captured".to_string()).chain(a))?;
        Ok(Self {fd,home,evidence,rpc,rpc_hash,check})
    }
}
fn publish(home:&std::path::Path, evidence:&std::path::Path, config:Validated, raw:&[u8],
    gate:impl FnOnce()->Result<()>) -> Result<()> {
    bootstrap::preserve_rpc(evidence,raw)?;
    gate()?;
    let engine=bootstrap::create(home,config,raw)?;
    drop(engine);
    Ok(())
}
fn run()->Result<()> {
    let i=Inputs::parse(std::env::args().skip(1))?;
    let channel=unsafe {
        let mut address:libc::sockaddr_storage=std::mem::zeroed();
        let mut len=std::mem::size_of_val(&address) as libc::socklen_t;
        let mut kind:libc::c_int=0; let mut kind_len=std::mem::size_of_val(&kind) as libc::socklen_t;
        if libc::getsockname(i.fd,&mut address as *mut _ as *mut libc::sockaddr,&mut len)!=0 ||
            address.ss_family as libc::c_int!=libc::AF_UNIX ||
            libc::getsockopt(i.fd,libc::SOL_SOCKET,libc::SO_TYPE,&mut kind as *mut _ as *mut _,&mut kind_len)!=0 || kind!=libc::SOCK_STREAM {
            return Err(Error::Invalid("START_GATE_FD"));
        }
        UnixStream::from_raw_fd(i.fd)
    };
    channel.peer_addr()?;
    let signals=signals::Signals::install().map_err(Error::Invalid)?;
    let config=i.check.validate(std::io::stdin().lock())?;
    let raw=capture_io::read_regular(&i.rpc,nus_exchange_contract::s3::evidence::limit(RPC)?)?;
    if sha256(&raw)!=i.rpc_hash { return Err(Error::Invalid("RPC_HASH")); }
    if signals.stop().load(Ordering::Relaxed) {return Err(Error::Invalid("BOOTSTRAP_STOPPED"));}
    publish(&i.home,&i.evidence,config,&raw,|| {
        start_gate::await_start(channel,signals.stop(),Duration::from_secs(5)).map_err(Error::Invalid)?;
        if signals.stop().load(Ordering::Relaxed) {return Err(Error::Invalid("BOOTSTRAP_STOPPED"));}
        Ok(())
    })?;
    std::io::stdout().lock().write_all(b"{\"home_created\":true,\"approval_verified\":false,\"service_started\":false,\"durable_ack\":false}\n")?;
    Ok(())
}
fn main()->std::process::ExitCode {
    std::panic::set_hook(Box::new(|_|{}));
    match std::panic::catch_unwind(run) {
        Ok(Ok(()))=>std::process::ExitCode::SUCCESS,
        _=>{eprintln!("LOCAL_BOOTSTRAP_CREATE_REJECTED");std::process::ExitCode::from(2)}
    }
}

#[cfg(test)] #[path="../../../exchange/tests/support/dev_fixture.rs"] mod fixture;
#[cfg(test)] mod create_tests {
    use super::*;
    use base64::{Engine as _,engine::general_purpose::STANDARD};
    use nus_exchange_contract::s3::{dev_local::Engine,journal::canonical};
    use std::os::unix::fs::PermissionsExt;
    #[test] fn create_gate_preserves_then_creates_and_reopens() {
        for fee in [0,25] { for allow in [false,true] {
            let (input,snapshot)=fixture::initial(fee); let home=fixture::home(fee);
            let evidence=home.parent().unwrap().join("evidence");std::fs::create_dir(&evidence).unwrap();
            std::fs::set_permissions(&evidence,std::fs::Permissions::from_mode(0o700)).unwrap();
            let raw=serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"response":{
                "code":0,"height":snapshot["height"],"value":STANDARD.encode(canonical(&snapshot).unwrap())}}})).unwrap();
            let result=publish(&home,&evidence,Validated::new(input.clone()).unwrap(),&raw,|| {
                assert!(!home.exists());assert_eq!(std::fs::read(evidence.join("bootstrap-rpc.json")).unwrap(),raw);
                if allow {Ok(())} else {Err(Error::Invalid("DENIED"))}
            });
            assert_eq!(result.is_ok(),allow);assert_eq!(home.exists(),allow);
            if allow {for _ in 0..2 {drop(Engine::open(&home,Validated::new(input.clone()).unwrap()).unwrap());}}
            assert!(publish(&home,&evidence,Validated::new(input).unwrap(),&raw,||panic!("no replace")).is_err());
            std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
        }}
    }
    #[test] fn real_child_ready_denial_preserves_without_home() {
        use std::{io::Read, os::{fd::AsRawFd,unix::process::CommandExt}};
        for fee in [0,25] {
            let (input,snapshot)=fixture::initial(fee); let home=fixture::home(fee);
            let root=home.parent().unwrap();let evidence=root.join("evidence");
            std::fs::create_dir(&evidence).unwrap();std::fs::set_permissions(&evidence,std::fs::Permissions::from_mode(0o700)).unwrap();
            let raw=serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"response":{
                "code":0,"height":snapshot["height"],"value":STANDARD.encode(canonical(&snapshot).unwrap())}}})).unwrap();
            let capture=canonical(&fixture::bundle(&input)).unwrap();
            let profile=root.join("profile");let rpc=root.join("rpc");let bundle=root.join("bundle");
            std::fs::write(&profile,&input.effective_profile).unwrap();std::fs::write(&rpc,&raw).unwrap();std::fs::write(&bundle,&capture).unwrap();
            let (child,mut parent)=UnixStream::pair().unwrap();parent.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let fd=child.as_raw_fd();let mut cmd=std::process::Command::new(std::env::var("NUS73_BOOTSTRAP_CREATE").unwrap());
            cmd.args(["create-captured","--start-gate-fd",&fd.to_string(),"--home",home.to_str().unwrap(),
                "--evidence-root",evidence.to_str().unwrap(),"--rpc-file",rpc.to_str().unwrap(),"--rpc-sha256",&sha256(&raw),
                "--capture-sha256",&sha256(&capture),"--runtime-pin",&input.approved_runtime_sha256,
                "--local-demo-profile",profile.to_str().unwrap(),"--acknowledge-unproven-space"])
                .stdin(std::fs::File::open(bundle).unwrap()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            unsafe {cmd.pre_exec(move||{if libc::fcntl(fd,libc::F_SETFD,0)<0 {Err(std::io::Error::last_os_error())} else {Ok(())}});}
            let mut process=cmd.spawn().unwrap();drop(child);
            let mut ready=[0;6];let received=parent.read_exact(&mut ready);
            if received.is_err() {process.kill().ok();process.wait().ok();panic!("READY failed");}
            assert_eq!(&ready,b"READY\n");assert!(!home.exists());
            assert_eq!(std::fs::read(evidence.join("bootstrap-rpc.json")).unwrap(),raw);
            drop(parent);let output=process.wait_with_output().unwrap();
            assert_eq!(output.status.code(),Some(2));assert!(output.stdout.is_empty());
            assert_eq!(output.stderr,b"LOCAL_BOOTSTRAP_CREATE_REJECTED\n");assert!(!home.exists());
            std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
        }
    }
    #[test] fn malformed_cli_rejects_before_stdin() {
        for args in [vec![],vec!["create"],vec!["create-captured","--start-gate-fd","2"],vec!["create-captured","--start-gate-fd","03"]] {
            assert!(Inputs::parse(args.clone().into_iter().map(String::from)).is_err());
            let mut child=std::process::Command::new(std::env::var("NUS73_BOOTSTRAP_CREATE").unwrap())
                .args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
            let deadline=std::time::Instant::now()+Duration::from_secs(2);
            while child.try_wait().unwrap().is_none() {
                if std::time::Instant::now()>=deadline {child.kill().unwrap();child.wait().unwrap();panic!("stdin wait");}
                std::thread::sleep(Duration::from_millis(5));
            }
            let out=child.wait_with_output().unwrap();assert_eq!(out.status.code(),Some(2));assert!(out.stdout.is_empty());
            assert_eq!(out.stderr,b"LOCAL_BOOTSTRAP_CREATE_REJECTED\n");
        }
    }
}

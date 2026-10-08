//! Internal L-T-only Snapshot fetch child. Parent must authenticate the exact
//! runtime before spawning, bound stdin/stdout/lifetime, preserve returned bytes,
//! and re-audit before create. This binary grants no organizational approval.
#[path = "bootstrap.rs"] mod bootstrap;
#[path = "bootstrap_check.rs"] mod check;
#[path = "signals.rs"] mod signals;
use nus_exchange_contract::s3::{dev_local::{Error, Result, Validated}, evidence};
use std::{io::{Read, Write}, net::SocketAddr, sync::atomic::Ordering};

struct Inputs { address: SocketAddr, check: check::Inputs }
impl Inputs {
    fn parse(args: impl IntoIterator<Item=String>) -> Result<Self> {
        let mut args=args.into_iter();
        if args.next().as_deref()!=Some("fetch-captured") ||
            args.next().as_deref()!=Some("--chain-rpc") {
            return Err(Error::Invalid("FETCH_ARGUMENTS"));
        }
        let raw=args.next().ok_or(Error::Invalid("FETCH_ADDRESS"))?;
        let address:SocketAddr=raw.parse().map_err(|_|Error::Invalid("FETCH_ADDRESS"))?;
        if !address.ip().is_loopback() || address.port()<1024 || address.to_string()!=raw {
            return Err(Error::Invalid("FETCH_ADDRESS"));
        }
        let check=check::Inputs::parse(std::iter::once("validate-captured".to_string()).chain(args))?;
        Ok(Self {address,check})
    }
}

fn fetch_with(inputs: Inputs, input: impl Read, mut output: impl Write,
    stopped: impl Fn()->bool,
    fetch: impl FnOnce(&Validated,SocketAddr)->Result<Vec<u8>>) -> Result<()> {
    let config=inputs.check.validate(input)?;
    if stopped() { return Err(Error::Invalid("FETCH_STOPPED")); }
    let raw=fetch(&config,inputs.address)?;
    // Never parse/reserialize RPC evidence. Preserve a completed response even
    // if stop arrived during IO; the parent must not use it as a create permit.
    if raw.is_empty() || raw.len()>evidence::limit(evidence::RPC)? {
        return Err(Error::Invalid("FETCH_RESPONSE_LIMIT"));
    }
    output.write_all(&raw)?;
    output.flush()?;
    Ok(())
}

fn run()->Result<()> {
    // Reject malformed CLI before waiting for stdin or creating a socket.
    let inputs=Inputs::parse(std::env::args().skip(1))?;
    let signals=signals::Signals::install().map_err(Error::Invalid)?;
    fetch_with(inputs,std::io::stdin().lock(),std::io::stdout().lock(),
        ||signals.stop().load(Ordering::Relaxed),bootstrap::fetch)
}
fn main()->std::process::ExitCode {
    std::panic::set_hook(Box::new(|_|{}));
    match std::panic::catch_unwind(run) {
        Ok(Ok(()))=>std::process::ExitCode::SUCCESS,
        _=>{eprintln!("LOCAL_BOOTSTRAP_FETCH_REJECTED");std::process::ExitCode::from(2)}
    }
}

#[cfg(test)] #[path="../../../exchange/tests/support/dev_fixture.rs"] mod fixture;
#[cfg(test)] mod fetch_tests {
    use super::*;
    use nus_exchange_contract::s3::journal::{canonical,sha256};
    fn args(profile:&std::path::Path,pin:&str,capture:&[u8])->Vec<String> {
        ["fetch-captured","--chain-rpc","127.0.0.1:26657","--capture-sha256",&sha256(capture),
         "--runtime-pin",pin,"--local-demo-profile",profile.to_str().unwrap(),
         "--acknowledge-unproven-space"].into_iter().map(String::from).collect()
    }
    #[test] fn validated_context_single_fetch_exact_bytes_and_cancellation() {
        for fee in [0,25] {
            let (input,_)=fixture::initial(fee);let home=fixture::home(fee);
            let profile=home.parent().unwrap().join("profile");
            std::fs::write(&profile,&input.effective_profile).unwrap();
            let capture=canonical(&fixture::bundle(&input)).unwrap();
            let expected=Validated::new(input.clone()).unwrap();
            let stopped=std::cell::Cell::new(false);let mut output=Vec::new();let mut calls=0;
            let raw=b"{ \"unparsed\": true }\n";
            fetch_with(Inputs::parse(args(&profile,&input.approved_runtime_sha256,&capture)).unwrap(),
                capture.as_slice(),&mut output,||stopped.get(),|config,address| {
                    calls+=1;assert_eq!(config.context(),expected.context());
                    assert_eq!(address.to_string(),"127.0.0.1:26657");
                    stopped.set(true);Ok(raw.to_vec())
                }).unwrap();
            assert_eq!(calls,1);assert_eq!(output,raw);assert!(!home.exists());
            output.clear();
            assert!(fetch_with(Inputs::parse(args(&profile,&input.approved_runtime_sha256,&capture)).unwrap(),
                capture.as_slice(),&mut output,||true,|_,_|panic!("stopped fetch")).is_err());
            assert!(output.is_empty());
            for response in [Err(Error::Invalid("QUERY_IO")),Ok(Vec::new()),Ok(vec![0;16_777_217])] {
                assert!(fetch_with(Inputs::parse(args(&profile,&input.approved_runtime_sha256,&capture)).unwrap(),
                    capture.as_slice(),&mut output,||false,|_,_|response).is_err());
                assert!(output.is_empty());assert!(!home.exists());
            }
            let mut changed=capture.clone();changed.push(b' ');
            assert!(fetch_with(Inputs::parse(args(&profile,&input.approved_runtime_sha256,&capture)).unwrap(),
                changed.as_slice(),&mut output,||false,|_,_|panic!("invalid capture fetch")).is_err());
            std::fs::remove_dir_all(home.ancestors().nth(4).unwrap()).unwrap();
        }
    }
    #[test] fn strict_cli_denies_before_reading_stdin() {
        let binary=std::env::var("NUS73_BOOTSTRAP_FETCH").unwrap();
        let base=args(std::path::Path::new("/profile"),&"a".repeat(64),b"capture");
        let mut cases=vec![vec![],vec!["serve".to_string()]];
        for address in ["localhost:26657","0.0.0.0:26657","127.0.0.1:80","127.0.0.1:026657"] {
            let mut v=base.clone();v[2]=address.to_string();cases.push(v);
        }
        let mut v=base.clone();v.pop();cases.push(v);
        let mut v=base.clone();v.push("--acknowledge-unproven-space".into());cases.push(v);
        for args in cases {
            let mut child=std::process::Command::new(&binary).args(args)
                .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped()).spawn().unwrap();
            let input=child.stdin.take().unwrap();let until=std::time::Instant::now()+std::time::Duration::from_secs(2);
            while child.try_wait().unwrap().is_none() {
                if std::time::Instant::now()>=until {child.kill().ok();child.wait().ok();panic!("stdin wait");}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            drop(input);let result=child.wait_with_output().unwrap();
            assert_eq!(result.status.code(),Some(2));assert!(result.stdout.is_empty());
            assert_eq!(result.stderr,b"LOCAL_BOOTSTRAP_FETCH_REJECTED\n");
        }
    }
}

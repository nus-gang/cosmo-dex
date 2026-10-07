//! Separate fault-build child. Parent must authenticate exact descriptor bytes
//! and approvals before START. No listener, REST, signing or broadcasting.
#[path="startup.rs"] mod startup;

#[path="signals.rs"] mod signals;
#[path="start_gate.rs"] mod start_gate;
use std::{os::fd::FromRawFd, os::unix::net::UnixStream, sync::atomic::Ordering, time::Duration};

fn run() -> Result<(), ()> {
    let mut args=std::env::args().skip(1).peekable();
    let apply=match args.next().as_deref() {
        Some("fault-seal-captured")=>false, Some("fault-apply-captured")=>true, _=>return Err(()) };
    if args.next().as_deref()!=Some("--start-gate-fd") { return Err(()); }
    let fd_raw=args.next().ok_or(())?;
    let fd=fd_raw.parse::<i32>().map_err(|_| ())?;
    if !(3..=1024).contains(&fd) || fd.to_string()!=fd_raw { return Err(()); }
    if args.next().as_deref()!=Some("--capture-sha256") { return Err(()); }
    let hash=args.next().ok_or(())?;
    let fault=startup::fault_options::Options::parse(&mut args).map_err(|_| ())?;
    if apply && fault.purpose != "NORMAL" { return Err(()); }
    let inputs=startup::Inputs::parse(args).map_err(|_| ())?;
    // Owned inherited AF_UNIX stream, never a public listener or pathname.
    let channel=unsafe {
        let mut address: libc::sockaddr_storage=std::mem::zeroed();
        let mut len=std::mem::size_of_val(&address) as libc::socklen_t;
        let mut kind: libc::c_int=0;
        let mut kind_len=std::mem::size_of_val(&kind) as libc::socklen_t;
        if libc::getsockname(fd, &mut address as *mut _ as *mut libc::sockaddr, &mut len)!=0 ||
           address.ss_family as libc::c_int != libc::AF_UNIX ||
           libc::getsockopt(fd,libc::SOL_SOCKET,libc::SO_TYPE,&mut kind as *mut _ as *mut _, &mut kind_len)!=0 ||
           kind != libc::SOCK_STREAM { return Err(()); }
        UnixStream::from_raw_fd(fd)
    };
    channel.local_addr().map_err(|_| ())?;
    channel.peer_addr().map_err(|_| ())?;
    let signals=signals::Signals::install().map_err(|_| ())?;
    let prepared=inputs.prepare_captured(std::io::stdin().lock(),&hash).map_err(|_| ())?;
    // READY is emitted only after the child owns the writer and private signer.
    start_gate::await_start(channel,signals.stop(),Duration::from_secs(5)).map_err(|_| ())?;
    if signals.stop().load(Ordering::Relaxed) { return Err(()); }
    let (command_succeeded,injected)=prepared.driver.fault_storage_once(apply,&fault.purpose,&fault.point,fault.occurrence,&fault.evidence,fault.errno.as_deref(),signals.stop()).map_err(|_| ())?;
    println!("{}",serde_json::json!({"schema":if apply {"s3-local-fault-apply-result/1"} else {"s3-local-fault-seal-result/1"},
        "command_succeeded":command_succeeded,"injected":injected,
        "durable_ack":false,"DEV":"NOT_RUN"}));
    Ok(())
}
fn main() -> std::process::ExitCode {
    // Fixed diagnostics even on unwind; never reveal keys, inputs or paths.
    std::panic::set_hook(Box::new(|_| {}));
    match std::panic::catch_unwind(run) {
        Ok(Ok(())) => std::process::ExitCode::SUCCESS,
        _ => { eprintln!("LOCAL_STORAGE_FAULT_REJECTED"); std::process::ExitCode::from(2) }
    }
}

//! Separate F09 fault child. Parent authenticates exact bytes and approvals.
//! No listener, REST, signing, broadcast, or Apply is started here.
#[path = "receipt_apply_options.rs"]
mod receipt_apply_options;
#[path = "signals.rs"]
mod signals;
#[path = "start_gate.rs"]
mod start_gate;
#[path = "startup.rs"]
mod startup;
use std::{os::fd::FromRawFd, os::unix::net::UnixStream, sync::atomic::Ordering, time::Duration};

fn run() -> Result<(), ()> {
    let mut args = std::env::args().skip(1).peekable();
    if args.next().as_deref() != Some("f09-crash-captured") {
        return Err(());
    }
    if args.next().as_deref() != Some("--start-gate-fd") {
        return Err(());
    }
    let fd_raw = args.next().ok_or(())?;
    let fd = fd_raw.parse::<i32>().map_err(|_| ())?;
    if !(3..=1024).contains(&fd) || fd.to_string() != fd_raw {
        return Err(());
    }
    if args.next().as_deref() != Some("--capture-sha256") {
        return Err(());
    }
    let hash = args.next().ok_or(())?;
    let fault = receipt_apply_options::Options::parse(&mut args).map_err(|_| ())?;
    let inputs = startup::Inputs::parse(args).map_err(|_| ())?;
    let channel = unsafe {
        let mut address: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of_val(&address) as libc::socklen_t;
        let mut kind: libc::c_int = 0;
        let mut kind_len = std::mem::size_of_val(&kind) as libc::socklen_t;
        if libc::getsockname(fd, &mut address as *mut _ as *mut libc::sockaddr, &mut len) != 0
            || address.ss_family as libc::c_int != libc::AF_UNIX
            || libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_TYPE,
                &mut kind as *mut _ as *mut _,
                &mut kind_len,
            ) != 0
            || kind != libc::SOCK_STREAM
        {
            return Err(());
        }
        UnixStream::from_raw_fd(fd)
    };
    channel.local_addr().map_err(|_| ())?;
    channel.peer_addr().map_err(|_| ())?;
    let signals = signals::Signals::install().map_err(|_| ())?;
    let prepared = inputs
        .prepare_captured(std::io::stdin().lock(), &hash)
        .map_err(|_| ())?;
    start_gate::await_start(channel, signals.stop(), Duration::from_secs(5)).map_err(|_| ())?;
    if signals.stop().load(Ordering::Relaxed) {
        return Err(());
    }
    prepared
        .driver
        .receipt_apply_crash_once(&fault.batch_id, &fault.evidence, signals.stop())
        .map_err(|_| ())?;
    Ok(())
}
fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    match std::panic::catch_unwind(run) {
        Ok(Ok(())) => std::process::ExitCode::SUCCESS,
        _ => {
            eprintln!("LOCAL_F09_REJECTED");
            std::process::ExitCode::from(2)
        }
    }
}

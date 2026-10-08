//! Dedicated offline executable. Deliberately has no service-start command.
#[path = "startup.rs"] mod startup;
use std::io::{Read, Write};

fn run(args: impl IntoIterator<Item=String>, input: impl Read) -> Result<(), ()> {
    let mut args=args.into_iter();
    if args.next().as_deref()!=Some("validate-captured") ||
       args.next().as_deref()!=Some("--capture-sha256") { return Err(()); }
    let hash=args.next().ok_or(())?;
    let inputs=startup::Inputs::parse(args).map_err(|_| ())?;
    let prepared=inputs.prepare_captured(input,&hash).map_err(|_| ())?;
    // Drop all signer/Engine ownership before reporting success. No bind/tick.
    drop(prepared);
    Ok(())
}
fn main() -> std::process::ExitCode {
    // Diagnostics never serialize input, filesystem paths, keys, or error chains.
    if run(std::env::args().skip(1),std::io::stdin().lock()).is_err() {
        eprintln!("LOCAL_PREFLIGHT_REJECTED");
        return std::process::ExitCode::from(2);
    }
    let report=b"{\"semantic_validation\":true,\"approval_verified\":false,\"service_started\":false,\"durable_ack\":false}\n";
    if std::io::stdout().lock().write_all(report).is_err() {
        return std::process::ExitCode::from(2);
    }
    std::process::ExitCode::SUCCESS
}

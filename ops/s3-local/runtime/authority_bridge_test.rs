//! Test-only bridge: Go publication -> real Rust signer -> Go SDK verification.
//! Never part of a runtime descriptor or service command.
#[path = "signer.rs"] mod signer;
use nus_exchange_contract::s3::settlement_local::chain::OperatorSigner;
use std::{io::Write, path::Path};
fn run() -> Result<(), ()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 { return Err(()); }
    let public = std::fs::read(&args[2]).map_err(|_| ())?;
    let key = signer::LocalSigner::load(Path::new(&args[1]), &public).map_err(|_| ())?;
    let sig = key.sign(b"NUS-73 fresh authority cross-language test").map_err(|_| ())?;
    std::io::stdout().write_all(&sig).map_err(|_| ())
}
fn main() { if run().is_err() { std::process::exit(2); } }

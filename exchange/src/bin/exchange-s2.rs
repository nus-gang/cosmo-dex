//! Parent-managed service; stdout is reserved exclusively for response frames.
use nus_exchange_contract::s2::runtime::{Manifest, Runtime, serve};
use std::{fs::File, io::Read, path::Path};
fn read(path: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("RESOURCE_LIMIT".into());
    }
    Ok(bytes)
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !((args.len() == 5 && args[1] == "create") || (args.len() == 4 && args[1] == "open")) {
        return Err(
            "usage: exchange-s2 create MANIFEST JOURNAL BOOTSTRAP | open MANIFEST JOURNAL".into(),
        );
    }
    let manifest = Manifest::decode(&read(&args[2])?)?;
    let initial = if args[1] == "create" {
        Some(read(&args[4])?)
    } else {
        None
    };
    let mut runtime = Runtime::start(&manifest, Path::new(&args[3]), initial.as_deref())?;
    serve(
        &mut runtime,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
    )?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        // Errors contain fixed codes/OS errors only; never log request bytes.
        eprintln!("exchange-s2: {error}");
        std::process::exit(1);
    }
}

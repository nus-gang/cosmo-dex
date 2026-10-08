//! Offline component driver. It opens no HTTP listener and broadcasts no TX.
use nus_exchange_contract::s3::dev_local::{Engine, Validated};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::Path,
};
fn read(p: &str, max: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut b = vec![];
    File::open(p)?.take(max as u64 + 1).read_to_end(&mut b)?;
    if b.len() > max {
        return Err("INPUT_SIZE".into());
    }
    Ok(b)
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or("create|open|validate required")?;
    let mut flags = BTreeMap::new();
    let mut ack = false;
    while let Some(k) = args.next() {
        if k == "--acknowledge-unproven-space" {
            if ack {
                return Err("DUPLICATE_OPTION".into());
            }
            ack = true;
            continue;
        }
        if ![
            "--local-demo-profile",
            "--input-set",
            "--runtime-pin",
            "--home",
            "--bootstrap",
        ]
        .contains(&k.as_str())
        {
            return Err("UNKNOWN_OPTION".into());
        }
        let v = args.next().ok_or("OPTION_VALUE_REQUIRED")?;
        if flags.insert(k, v).is_some() {
            return Err("DUPLICATE_OPTION".into());
        }
    }
    let get = |k: &str| flags.get(k).map(String::as_str).ok_or("OPTION_REQUIRED");
    if !ack || !flags.contains_key("--local-demo-profile") {
        return Err("LOCAL_DEMO_OPT_IN_REQUIRED".into());
    }
    let c = Validated::decode_bundle(
        &read(get("--input-set")?, 48 * 1024 * 1024)?,
        read(get("--local-demo-profile")?, 1024 * 1024)?,
        get("--runtime-pin")?.into(),
        ack,
    )?;
    if mode == "validate" {
        println!("VALIDATED_INPUT_BYTES_ONLY; RUNTIME_APPROVAL_NOT_ESTABLISHED; durable_ack=false");
        return Ok(());
    }
    let engine = match mode.as_str() {
        "create" => Engine::create(
            Path::new(get("--home")?),
            c,
            &read(get("--bootstrap")?, 262144)?,
        )?,
        "open" if !flags.contains_key("--bootstrap") => Engine::open(Path::new(get("--home")?), c)?,
        _ => return Err("INVALID_MODE".into()),
    };
    let view = engine.reader().get()?;
    serde_json::to_writer(
        std::io::stdout().lock(),
        &serde_json::json!({"scope":"OFFLINE_COMPONENT","durable_ack":false,"gate":view.gate,"command_seq":view.commit.command_seq.to_string(),"record_hash":view.commit.record_hash}),
    )?;
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("nus-s3-local-demo: {e}");
        std::process::exit(1);
    }
}

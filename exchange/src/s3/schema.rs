//! Pinned s3/3 structural validation. Semantic authority remains with the local
//! adapter and the sequencer. No permissive S2/rc1 storage migration exists.
use super::journal::{canonical, sha256};
use crate::{Result, codec};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::sync::LazyLock;
pub static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../protocol/s3/schema.json"))
        .expect("pinned S3 schema")
});
pub const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
pub fn bytes(v: &Value) -> Result<Vec<u8>> {
    let s = v.as_str().ok_or("S3_BYTES")?;
    let raw = STANDARD.decode(s).map_err(|_| "S3_BYTES")?;
    if STANDARD.encode(&raw) != s {
        return Err("S3_BYTES");
    }
    Ok(raw)
}
pub fn num(v: &Value) -> Result<u64> {
    Ok(codec::integer(v, 64)? as u64)
}
pub fn hash(domain: &str, v: &Value) -> Result<String> {
    Ok(sha256(&codec::frame(
        domain,
        &canonical(v).map_err(|_| "S3_CANONICAL")?,
    )))
}
pub fn validate(name: &str, v: &Value) -> Result<()> {
    if SCHEMA["$defs"].get(name).is_none() {
        return Err("S3_SCHEMA");
    }
    match name {
        "U32" => {
            codec::integer(v, 32)?;
        }
        "U64" => {
            codec::integer(v, 64)?;
        }
        "Atoms" => {
            codec::integer(v, 128)?;
        }
        "Hash" => {
            let s = v.as_str().ok_or("S3_HASH")?;
            if s.len() != 64
                || !s
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err("S3_HASH");
            }
        }
        "Bytes" => {
            bytes(v)?;
        }
        _ => {}
    }
    check(&SCHEMA["$defs"][name], v)
}
fn check(s: &Value, v: &Value) -> Result<()> {
    if let Some(r) = s["$ref"].as_str() {
        return validate(r.strip_prefix("#/$defs/").ok_or("S3_SCHEMA")?, v);
    }
    if let Some(a) = s["anyOf"].as_array() {
        return if a.iter().any(|s| check(s, v).is_ok()) {
            Ok(())
        } else {
            Err("S3_SCHEMA")
        };
    }
    if s.get("const").is_some_and(|x| x != v)
        || s["enum"].as_array().is_some_and(|a| !a.contains(v))
    {
        return Err("S3_SCHEMA");
    }
    match s["type"].as_str() {
        Some("object") => {
            let o = v.as_object().ok_or("S3_SCHEMA")?;
            let p = s["properties"].as_object().ok_or("S3_SCHEMA")?;
            let required = s["required"].as_array().ok_or("S3_SCHEMA")?;
            if o.len() != p.len()
                || required
                    .iter()
                    .any(|k| !o.contains_key(k.as_str().unwrap()))
            {
                return Err("S3_SCHEMA");
            }
            for (k, v) in o {
                check(p.get(k).ok_or("S3_SCHEMA")?, v)?;
            }
        }
        Some("array") => {
            let a = v.as_array().ok_or("S3_SCHEMA")?;
            if s["maxItems"].as_u64().is_some_and(|m| a.len() as u64 > m) {
                return Err("S3_SCHEMA");
            }
            for v in a {
                check(&s["items"], v)?;
            }
        }
        Some("string") => {
            let a = v.as_str().ok_or("S3_SCHEMA")?;
            if s["maxLength"]
                .as_u64()
                .is_some_and(|m| a.chars().count() as u64 > m)
            {
                return Err("S3_SCHEMA");
            }
            match s["pattern"].as_str() {
                Some("^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$") => {
                    bytes(v)?;
                }
                Some("^(0|[1-9][0-9]*)$") | Some("^[0-9a-f]{64}$") | None => {}
                _ => return Err("S3_SCHEMA_PATTERN"),
            }
        }
        Some("boolean") if v.is_boolean() => {}
        Some("null") if v.is_null() => {}
        None if s.get("const").is_some() => {}
        _ => return Err("S3_SCHEMA"),
    }
    Ok(())
}
pub fn decode(name: &str, raw: &[u8]) -> Result<Value> {
    if raw.len() > super::journal::MAX_PAYLOAD {
        return Err("RESOURCE_LIMIT");
    }
    let v = codec::unique_json(raw)?;
    validate(name, &v)?;
    Ok(v)
}

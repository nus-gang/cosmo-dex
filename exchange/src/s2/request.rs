//! Bounded REST body decoding. This is not authentication: the caller must
//! verify the session and actual Origin before invoking Service with its owner.
use crate::{
    Result,
    codec::{self, Codec},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

pub const MAX_REQUEST_BYTES: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignedKind {
    Order,
    Cancel,
}
impl SignedKind {
    pub fn command(self) -> &'static str {
        match self {
            Self::Order => "ORDER",
            Self::Cancel => "CANCEL",
        }
    }
    fn wire_type(self) -> &'static str {
        match self {
            Self::Order => "OrderV1",
            Self::Cancel => "CancelV1",
        }
    }
}

// Do not derive Debug: signed input should not leak into routine request logs.
pub struct SignedCommand {
    pub kind: SignedKind,
    pub wire: Vec<u8>,
    pub signature: Vec<u8>,
}
fn object(raw: &[u8], keys: &[&str]) -> Result<Value> {
    if raw.len() > MAX_REQUEST_BYTES {
        return Err("RESOURCE_LIMIT");
    }
    let value = codec::unique_json(raw).map_err(|_| "NON_CANONICAL_WIRE")?;
    let obj = value.as_object().ok_or("NON_CANONICAL_WIRE")?;
    if obj.len() != keys.len() || keys.iter().any(|key| !obj.contains_key(*key)) {
        return Err("NON_CANONICAL_WIRE");
    }
    Ok(value)
}
fn bytes(value: &Value) -> Result<Vec<u8>> {
    let text = value.as_str().ok_or("NON_CANONICAL_WIRE")?;
    let decoded = STANDARD.decode(text).map_err(|_| "NON_CANONICAL_WIRE")?;
    if STANDARD.encode(&decoded) != text {
        return Err("NON_CANONICAL_WIRE");
    }
    Ok(decoded)
}
impl SignedCommand {
    /// expected_context must be the context of the manifest-validated engine
    /// snapshot, never supplied by the request. Signature verification remains
    /// in the sequencer, using the registered key and authenticated owner.
    pub fn decode(kind: SignedKind, raw: &[u8], expected_context: &Value) -> Result<Self> {
        let value = object(raw, &["context", "wire_base64", "signature_base64"])?;
        let wire = bytes(&value["wire_base64"])?;
        let signature = bytes(&value["signature_base64"])?;
        if signature.len() != 3309 {
            return Err("NON_CANONICAL_WIRE");
        }
        Codec::default().decode(kind.wire_type(), &wire)?;
        if &value["context"] != expected_context {
            return Err("CONTEXT_MISMATCH");
        }
        Ok(Self {
            kind,
            wire,
            signature,
        })
    }
}

/// Return canonical LocalAction bytes for the existing owner/kind/ID binding.
/// No owner field is accepted and no monetary transfer is authorized here.
pub fn local_action(raw: &[u8]) -> Result<Vec<u8>> {
    let value = object(raw, &["request_id"])?;
    let id = value["request_id"].as_str().ok_or("NON_CANONICAL_WIRE")?;
    if id.len() != 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("NON_CANONICAL_WIRE");
    }
    super::journal::canonical(&value).map_err(|_| "NON_CANONICAL_WIRE")
}

/// Only the approved S2 loopback profile; the caller must reject duplicate
/// Origin headers before passing their single value. Not a production policy.
pub fn mutation_origin(origin: Option<&str>) -> Result<&str> {
    match origin {
        Some(value @ ("http://127.0.0.1:5173" | "http://localhost:5173")) => Ok(value),
        _ => Err("FORBIDDEN"),
    }
}

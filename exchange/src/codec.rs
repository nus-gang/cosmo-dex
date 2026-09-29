use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use fips204::{
    ml_dsa_65,
    traits::{SerDes, Verifier},
};
use serde::{
    Deserialize, Deserializer,
    de::{Error, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt};

pub fn integer(v: &Value, bits: u32) -> Result<u128> {
    let s = v.as_str().ok_or("INTEGER_RANGE")?;
    if s.is_empty() || (s.len() > 1 && s.starts_with('0')) || !s.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("INTEGER_RANGE");
    }
    let n = s.parse::<u128>().map_err(|_| "INTEGER_RANGE")?;
    if bits < 128 && n >= 1u128 << bits {
        return Err("INTEGER_RANGE");
    }
    Ok(n)
}

// serde_json::Value alone silently overwrites duplicate keys. Reject them at every depth.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON with unique keys")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut m: M,
            ) -> std::result::Result<Unique, M::Error> {
                let mut o = Map::new();
                while let Some((k, Unique(v))) = m.next_entry::<String, Unique>()? {
                    if o.insert(k, v).is_some() {
                        return Err(M::Error::custom("duplicate key"));
                    }
                }
                Ok(Unique(Value::Object(o)))
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut s: S,
            ) -> std::result::Result<Unique, S::Error> {
                let mut a = vec![];
                while let Some(Unique(v)) = s.next_element()? {
                    a.push(v)
                }
                Ok(Unique(Value::Array(a)))
            }
            fn visit_str<E: Error>(self, s: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::String(s.into())))
            }
            fn visit_bool<E: Error>(self, b: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Bool(b)))
            }
            fn visit_u64<E: Error>(self, n: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::from(n)))
            }
            fn visit_i64<E: Error>(self, n: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::from(n)))
            }
            fn visit_f64<E: Error>(self, n: f64) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::from(n)))
            }
            fn visit_unit<E: Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
        }
        d.deserialize_any(V)
    }
}
#[derive(Deserialize)]
struct Field {
    tag: u64,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    repeated: bool,
}
pub struct Codec {
    schema: BTreeMap<String, Vec<Field>>,
}
impl Default for Codec {
    fn default() -> Self {
        Self {
            schema: serde_json::from_str(include_str!("../../protocol/v1/schema.json"))
                .expect("pinned schema"),
        }
    }
}
fn max_bytes(name: &str) -> usize {
    match name {
        "OrderV1" => 8192,
        "CancelV1" | "WalletChallengeV1" => 1024,
        _ => 1048576,
    }
}
fn text_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(&c))
}
fn width(t: &str) -> Option<usize> {
    match t {
        "h" => Some(32),
        "a" => Some(20),
        "pk" => Some(1952),
        "sig" => Some(3309),
        "atoms" => Some(16),
        _ => None,
    }
}
pub fn varint(mut n: u64, out: &mut Vec<u8>) {
    loop {
        let b = (n & 127) as u8;
        n >>= 7;
        out.push(b | if n > 0 { 128 } else { 0 });
        if n == 0 {
            break;
        }
    }
}
fn read_varint(raw: &[u8], p: &mut usize) -> Result<u64> {
    let mut n = 0;
    for i in 0..10 {
        let b = *raw.get(*p).ok_or("NON_CANONICAL_WIRE")?;
        *p += 1;
        if i == 9 && b > 1 {
            return Err("NON_CANONICAL_WIRE");
        }
        n |= ((b & 127) as u64) << (7 * i);
        if b < 128 {
            if i > 0 && b == 0 {
                return Err("NON_CANONICAL_WIRE");
            }
            return Ok(n);
        }
    }
    Err("NON_CANONICAL_WIRE")
}
impl Codec {
    pub fn encode_json(&self, name: &str, json: &str) -> Result<Vec<u8>> {
        if json.len() > 2 * 1048576 {
            return Err("RESOURCE_LIMIT");
        }
        let Unique(v) = serde_json::from_str(json).map_err(|_| "NON_CANONICAL_WIRE")?;
        self.encode(name, &v)
    }
    pub fn encode(&self, name: &str, v: &Value) -> Result<Vec<u8>> {
        self.encode_at(name, v, 0)
    }
    fn encode_at(&self, name: &str, v: &Value, depth: usize) -> Result<Vec<u8>> {
        if depth > 4 {
            return Err("RESOURCE_LIMIT");
        }
        let fields = self.schema.get(name).ok_or("NON_CANONICAL_WIRE")?;
        let obj = v.as_object().ok_or("NON_CANONICAL_WIRE")?;
        if obj.len() != fields.len() || obj.keys().any(|k| !fields.iter().any(|f| &f.name == k)) {
            return Err("NON_CANONICAL_WIRE");
        }
        let mut out = vec![];
        for f in fields {
            let v = obj.get(&f.name).ok_or("NON_CANONICAL_WIRE")?;
            let values = if f.repeated {
                v.as_array()
                    .ok_or("NON_CANONICAL_WIRE")?
                    .iter()
                    .collect::<Vec<_>>()
            } else {
                vec![v]
            };
            if f.repeated && values.len() > if f.name == "fills" { 1000 } else { 100 } {
                return Err("RESOURCE_LIMIT");
            }
            for v in values {
                if f.kind == "u32" || f.kind == "u64" {
                    let n = integer(v, if f.kind == "u32" { 32 } else { 64 })?;
                    varint(f.tag << 3, &mut out);
                    varint(n as u64, &mut out);
                    continue;
                }
                let b = if self.schema.contains_key(&f.kind) {
                    self.encode_at(&f.kind, v, depth + 1)?
                } else if f.kind == "atoms" {
                    integer(v, 128)?.to_be_bytes().to_vec()
                } else {
                    let s = v.as_str().ok_or("NON_CANONICAL_WIRE")?;
                    match f.kind.as_str() {
                        "h" => {
                            if s.len() != 64
                                || !s
                                    .bytes()
                                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                            {
                                return Err("NON_CANONICAL_WIRE");
                            }
                            hex::decode(s).map_err(|_| "NON_CANONICAL_WIRE")?
                        }
                        "a" | "pk" | "sig" => {
                            let b = STANDARD.decode(s).map_err(|_| "NON_CANONICAL_WIRE")?;
                            if STANDARD.encode(&b) != s {
                                return Err("NON_CANONICAL_WIRE");
                            }
                            b
                        }
                        "s" => {
                            if !text_ok(s) {
                                return Err("NON_CANONICAL_WIRE");
                            }
                            s.as_bytes().to_vec()
                        }
                        _ => return Err("NON_CANONICAL_WIRE"),
                    }
                };
                if width(&f.kind).is_some_and(|w| w != b.len()) {
                    return Err("NON_CANONICAL_WIRE");
                }
                varint((f.tag << 3) | 2, &mut out);
                varint(b.len() as u64, &mut out);
                out.extend(b);
                if out.len() > max_bytes(name) {
                    return Err("RESOURCE_LIMIT");
                }
            }
        }
        if out.len() > max_bytes(name) {
            return Err("RESOURCE_LIMIT");
        }
        Ok(out)
    }
    pub fn decode(&self, name: &str, raw: &[u8]) -> Result<Value> {
        let v = self.decode_at(name, raw, 0)?;
        if self.encode(name, &v)? != raw {
            return Err("NON_CANONICAL_WIRE");
        }
        Ok(v)
    }
    fn decode_at(&self, name: &str, raw: &[u8], depth: usize) -> Result<Value> {
        if raw.len() > max_bytes(name) || depth > 4 {
            return Err("RESOURCE_LIMIT");
        }
        let fields = self.schema.get(name).ok_or("NON_CANONICAL_WIRE")?;
        let mut obj = Map::new();
        for f in fields.iter().filter(|f| f.repeated) {
            obj.insert(f.name.clone(), Value::Array(vec![]));
        }
        let (mut p, mut last) = (0, 0);
        while p < raw.len() {
            let key = read_varint(raw, &mut p)?;
            let tag = key >> 3;
            let wire = key & 7;
            let f = fields
                .iter()
                .find(|f| f.tag == tag)
                .ok_or("NON_CANONICAL_WIRE")?;
            if tag < last || (!f.repeated && obj.contains_key(&f.name)) {
                return Err("NON_CANONICAL_WIRE");
            }
            last = tag;
            let v = if f.kind == "u32" || f.kind == "u64" {
                if wire != 0 {
                    return Err("NON_CANONICAL_WIRE");
                }
                let n = read_varint(raw, &mut p)?;
                if f.kind == "u32" && n > u32::MAX as u64 {
                    return Err("INTEGER_RANGE");
                }
                Value::String(n.to_string())
            } else {
                if wire != 2 {
                    return Err("NON_CANONICAL_WIRE");
                }
                let len =
                    usize::try_from(read_varint(raw, &mut p)?).map_err(|_| "NON_CANONICAL_WIRE")?;
                let end = p
                    .checked_add(len)
                    .filter(|e| *e <= raw.len())
                    .ok_or("NON_CANONICAL_WIRE")?;
                let b = &raw[p..end];
                p = end;
                if self.schema.contains_key(&f.kind) {
                    self.decode_at(&f.kind, b, depth + 1)?
                } else {
                    if width(&f.kind).is_some_and(|w| w != len) {
                        return Err("NON_CANONICAL_WIRE");
                    }
                    Value::String(match f.kind.as_str() {
                        "h" => hex::encode(b),
                        "a" | "pk" | "sig" => STANDARD.encode(b),
                        "atoms" => {
                            u128::from_be_bytes(b.try_into().map_err(|_| "NON_CANONICAL_WIRE")?)
                                .to_string()
                        }
                        "s" => {
                            let s = std::str::from_utf8(b).map_err(|_| "NON_CANONICAL_WIRE")?;
                            if !text_ok(s) {
                                return Err("NON_CANONICAL_WIRE");
                            }
                            s.into()
                        }
                        _ => return Err("NON_CANONICAL_WIRE"),
                    })
                }
            };
            if f.repeated {
                let a = obj.get_mut(&f.name).unwrap().as_array_mut().unwrap();
                a.push(v);
                if a.len() > if f.name == "fills" { 1000 } else { 100 } {
                    return Err("RESOURCE_LIMIT");
                }
            } else {
                obj.insert(f.name.clone(), v);
            }
        }
        if obj.len() != fields.len() {
            return Err("NON_CANONICAL_WIRE");
        }
        Ok(Value::Object(obj))
    }
}
pub fn frame(domain: &str, body: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    out.extend((domain.len() as u32).to_be_bytes());
    out.extend(domain.as_bytes());
    out.extend((body.len() as u64).to_be_bytes());
    out.extend(body);
    out
}
pub fn hash(raw: &[u8]) -> [u8; 32] {
    Sha256::digest(raw).into()
}
pub fn verify_raw(pk: &[u8], message: &[u8], sig: &[u8], context: &[u8]) -> bool {
    if !context.is_empty() {
        return false;
    }
    let (Ok(pk), Ok(sig)) = (pk.try_into(), sig.try_into()) else {
        return false;
    };
    ml_dsa_65::PublicKey::try_from_bytes(pk).is_ok_and(|p| p.verify(message, &sig, context))
}
pub fn address(pk: &[u8]) -> Result<[u8; 20]> {
    if pk.len() != 1952 {
        return Err("KEY_LENGTH");
    }
    Ok(hash(pk)[..20].try_into().unwrap())
}
pub fn decode_address(s: &str) -> Result<[u8; 20]> {
    use bech32::{FromBase32, ToBase32, Variant};
    let (hrp, data, variant) = bech32::decode(s).map_err(|_| "ADDRESS_MISMATCH")?;
    let b = Vec::<u8>::from_base32(&data).map_err(|_| "ADDRESS_MISMATCH")?;
    if hrp != "nus"
        || variant != Variant::Bech32
        || bech32::encode("nus", b.to_base32(), Variant::Bech32).map_err(|_| "ADDRESS_MISMATCH")?
            != s
    {
        return Err("ADDRESS_MISMATCH");
    }
    b.try_into().map_err(|_| "ADDRESS_MISMATCH")
}

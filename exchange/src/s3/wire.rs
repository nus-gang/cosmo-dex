//! Strict batch V2 uses the unchanged V1 field layout, with version/domain 2.
//! SDK envelope inspection binds worker attempts to the exact immutable batch.
use super::{
    journal::sha256,
    schema::{self, num},
};
use crate::{
    Result,
    codec::{self, Codec},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
#[derive(Clone, Debug)]
pub(crate) struct Field<'a> {
    pub tag: u64,
    pub number: Option<u64>,
    pub data: &'a [u8],
    pub encoded: &'a [u8],
}
fn varint(raw: &[u8], offset: &mut usize) -> Result<u64> {
    let start = *offset;
    let mut n = 0;
    for i in 0..10 {
        let b = *raw.get(*offset).ok_or("INVALID_ENVELOPE")?;
        *offset += 1;
        if i == 9 && b > 1 {
            return Err("INVALID_ENVELOPE");
        }
        n |= u64::from(b & 127) << (7 * i);
        if b & 128 == 0 {
            if *offset - start > 1 && b == 0 {
                return Err("INVALID_ENVELOPE");
            }
            return Ok(n);
        }
    }
    Err("INVALID_ENVELOPE")
}
pub(crate) fn fields(raw: &[u8]) -> Result<Vec<Field<'_>>> {
    let mut out = vec![];
    let mut p = 0;
    let mut prior = 0;
    while p < raw.len() {
        let start = p;
        let key = varint(raw, &mut p)?;
        let tag = key >> 3;
        if tag == 0 || tag < prior {
            return Err("INVALID_ENVELOPE");
        }
        prior = tag;
        let (number, data) = match key & 7 {
            0 => (Some(varint(raw, &mut p)?), &raw[p..p]),
            2 => {
                let n = usize::try_from(varint(raw, &mut p)?).map_err(|_| "INVALID_ENVELOPE")?;
                let end = p.checked_add(n).ok_or("INVALID_ENVELOPE")?;
                let d = raw.get(p..end).ok_or("INVALID_ENVELOPE")?;
                p = end;
                (None, d)
            }
            _ => return Err("INVALID_ENVELOPE"),
        };
        out.push(Field {
            tag,
            number,
            data,
            encoded: &raw[start..p],
        });
    }
    Ok(out)
}
fn one<'a>(f: &'a [Field<'a>], tag: u64) -> Result<&'a Field<'a>> {
    let mut i = f.iter().filter(|f| f.tag == tag);
    let a = i.next().ok_or("INVALID_ENVELOPE")?;
    if i.next().is_some() {
        return Err("INVALID_ENVELOPE");
    }
    Ok(a)
}
fn exact(f: &[Field<'_>], tags: &[u64]) -> Result<()> {
    if f.len() != tags.len() || f.iter().zip(tags).any(|(f, t)| f.tag != *t) {
        return Err("INVALID_ENVELOPE");
    }
    Ok(())
}
fn optional_number(f: &[Field<'_>], tag: u64) -> Result<u64> {
    if !f.iter().any(|f| f.tag == tag) {
        return Ok(0);
    }
    one(f, tag)?.number.ok_or("INVALID_ENVELOPE")
}
pub fn seal(mut v: Value) -> Result<(Vec<u8>, Value)> {
    v["batch_id"] = json!(schema::ZERO);
    let initial = Codec::default().encode("BatchV1", &v)?;
    let core: Vec<u8> = fields(&initial)?
        .into_iter()
        .filter(|f| f.tag != 7)
        .flat_map(|f| f.encoded.to_vec())
        .collect();
    v["batch_id"] = json!(sha256(&codec::frame("NUS/BATCH_ID/V2", &core)));
    let raw = Codec::default().encode("BatchV1", &v)?;
    let id = identity(&raw)?;
    Ok((raw, id))
}
pub fn identity(raw: &[u8]) -> Result<Value> {
    if raw.len() > 131072 {
        return Err("RESOURCE_LIMIT");
    }
    let v = Codec::default().decode("BatchV1", raw)?;
    let fills = v["fills"].as_array().unwrap();
    let orders = v["new_signed_orders"].as_array().unwrap();
    if v["protocol_version"] != "2"
        || fills.is_empty()
        || fills.len() > 8
        || orders.is_empty()
        || orders.len() > 16
    {
        return Err("BATCH_LIMIT");
    }
    let core: Vec<u8> = fields(raw)?
        .into_iter()
        .filter(|f| f.tag != 7)
        .flat_map(|f| f.encoded.to_vec())
        .collect();
    if sha256(&codec::frame("NUS/BATCH_ID/V2", &core)) != v["batch_id"] {
        return Err("BATCH_CONFLICT");
    }
    Ok(
        json!({"operator_epoch":v["operator_epoch"],"batch_seq":v["batch_seq"],"batch_id":v["batch_id"],"batch_hash":sha256(&codec::frame("NUS/BATCH_HASH/V2",raw)),"previous_batch_hash":v["previous_batch_hash"],"fill_ids":fills.iter().map(|f|f["fill_id"].clone()).collect::<Vec<_>>()}),
    )
}
/// Returns close audit bindings, or None for a settle envelope. Signature
/// authorization executes on chain; no operator secret is required here.
pub fn attempt_envelope(
    a: &Value,
    objects: &super::evidence::Objects,
    batch_wire: &[u8],
) -> Result<Option<(String, String)>> {
    let raw = objects.resolve(&a["raw_tx_ref"], super::evidence::TX)?;
    if raw.len() > 139264 || sha256(raw) != a["tx_hash"] {
        return Err("INVALID_ENVELOPE");
    }
    let tx = fields(raw)?;
    exact(&tx, &[1, 2, 3])?;
    if one(&tx, 3)?.data.len() != 3309 {
        return Err("KEY_LENGTH");
    }
    let body = fields(one(&tx, 1)?.data)?;
    exact(&body, &[1, 3])?;
    if one(&body, 3)?.number != Some(num(&a["timeout_height"])?) {
        return Err("INVALID_ENVELOPE");
    }
    let any = fields(one(&body, 1)?.data)?;
    exact(&any, &[1, 2])?;
    let kind = a["kind"].as_str().ok_or("INVALID_ENVELOPE")?;
    let expected = if kind == "SETTLE" {
        "/nus.exchange.s3.v1.MsgSettleBatch"
    } else {
        "/nus.exchange.s3.v1.MsgCloseBatch"
    };
    if one(&any, 1)?.data != expected.as_bytes() {
        return Err("INVALID_ENVELOPE");
    }
    let msg = fields(one(&any, 2)?.data)?;
    exact(
        &msg,
        if kind == "SETTLE" {
            &[1, 2]
        } else {
            &[1, 2, 3, 4]
        },
    )?;
    let addr = std::str::from_utf8(one(&msg, 1)?.data).map_err(|_| "INVALID_ENVELOPE")?;
    let (hrp, data, variant) = bech32::decode(addr).map_err(|_| "ADDRESS_MISMATCH")?;
    use bech32::FromBase32;
    let owner = Vec::<u8>::from_base32(&data).map_err(|_| "ADDRESS_MISMATCH")?;
    if hrp != "nus"
        || variant != bech32::Variant::Bech32
        || addr != addr.to_lowercase()
        || STANDARD.encode(&owner) != a["operator"]
        || one(&msg, 2)?.data != batch_wire
    {
        return Err("BATCH_CONFLICT");
    }
    let auth = fields(one(&tx, 2)?.data)?;
    exact(&auth, &[1, 2])?;
    let signer = fields(one(&auth, 1)?.data)?;
    if !(signer.iter().map(|f| f.tag).eq([1, 2]) || signer.iter().map(|f| f.tag).eq([1, 2, 3]))
        || optional_number(&signer, 3)? != num(&a["account_sequence"])?
    {
        return Err("INVALID_ENVELOPE");
    }
    let pkany = fields(one(&signer, 1)?.data)?;
    exact(&pkany, &[1, 2])?;
    if one(&pkany, 1)?.data != b"/cosmos.crypto.mldsa65.PubKey" {
        return Err("KEY_LENGTH");
    }
    let pk = fields(one(&pkany, 2)?.data)?;
    exact(&pk, &[1])?;
    if one(&pk, 1)?.data.len() != 1952
        || hex::decode(sha256(one(&pk, 1)?.data)).map_err(|_| "KEY_LENGTH")?[..20] != owner
    {
        return Err("ADDRESS_MISMATCH");
    }
    let mode = fields(one(&signer, 2)?.data)?;
    exact(&mode, &[1])?;
    let single = fields(one(&mode, 1)?.data)?;
    exact(&single, &[1])?;
    if one(&single, 1)?.number != Some(1) {
        return Err("INVALID_ENVELOPE");
    }
    let fee = fields(one(&auth, 2)?.data)?;
    exact(&fee, &[1, 2])?;
    let coin = fields(one(&fee, 1)?.data)?;
    exact(&coin, &[1, 2])?;
    if one(&coin, 1)?.data != b"DEVGAS"
        || one(&coin, 2)?.data
            != a["fee_atoms"]
                .as_str()
                .ok_or("INVALID_ENVELOPE")?
                .as_bytes()
        || one(&fee, 2)?.number != Some(num(&a["gas_limit"])?)
    {
        return Err("INVALID_ENVELOPE");
    }
    if kind == "SETTLE" {
        Ok(None)
    } else {
        if one(&msg, 3)?.data.len() != 32 || one(&msg, 4)?.data.len() != 32 {
            return Err("INVALID_ENVELOPE");
        }
        Ok(Some((
            hex::encode(one(&msg, 3)?.data),
            hex::encode(one(&msg, 4)?.data),
        )))
    }
}

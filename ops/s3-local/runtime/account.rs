//! Height-bound auth Account decoding for the trusted local B RPC only.
//! SDK v0.55.0 QueryAccountResponse -> Any -> BaseAccount. No key logging.
use bech32::ToBase32;
use nus_exchange_contract::{
    codec,
    s3::{
        dev_local::{Error, Result},
        evidence, schema,
        snapshot::Snapshot,
    },
};
use serde_json::Value;

pub struct Account {
    snapshot_id: String,
    owner: Vec<u8>,
    number: u64,
    sequence: u64,
    raw: Vec<u8>,
}
impl Account {
    pub fn at(&self, snapshot: &Snapshot, owner: &[u8]) -> Result<(u64, u64)> {
        if self.snapshot_id != snapshot.id() || self.owner != owner {
            return Err(Error::Invalid("ACCOUNT_BINDING"));
        }
        Ok((self.number, self.sequence))
    }
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }
}
fn address(owner: &[u8]) -> Result<String> {
    if owner.len() != 20 {
        return Err(Error::Invalid("ACCOUNT_OWNER"));
    }
    Ok(
        bech32::encode("nus", owner.to_base32(), bech32::Variant::Bech32)
            .map_err(|_| "ACCOUNT_OWNER")?,
    )
}
pub fn request(owner: &[u8]) -> Result<Vec<u8>> {
    let addr = address(owner)?;
    // A 20-byte nus address fits a one-byte protobuf length.
    Ok([vec![10, addr.len() as u8], addr.into_bytes()].concat())
}
// Strict bounded protobuf parser: reject duplicate/unknown fields, wrong wire
// types, nonminimal integers, overflowing lengths and truncation. Zero scalar
// fields may be omitted by the SDK. This supports BaseAccount only.
fn var(raw: &mut &[u8]) -> Result<u64> {
    let mut n = 0u64;
    for i in 0..10 {
        let (&b, rest) = raw.split_first().ok_or("ACCOUNT_PROTO")?;
        *raw = rest;
        if i == 9 && b > 1 {
            return Err(Error::Invalid("ACCOUNT_PROTO"));
        }
        n |= u64::from(b & 127) << (7 * i);
        if b < 128 {
            if i > 0 && b == 0 {
                return Err(Error::Invalid("ACCOUNT_PROTO"));
            }
            return Ok(n);
        }
    }
    Err(Error::Invalid("ACCOUNT_PROTO"))
}
struct Fields<'a> {
    blobs: [Option<&'a [u8]>; 5],
    nums: [u64; 5],
}
fn fields<'a>(mut raw: &'a [u8], types: &[u8]) -> Result<Fields<'a>> {
    let mut out = Fields {
        blobs: [None; 5],
        nums: [0; 5],
    };
    let mut seen = [false; 5];
    while !raw.is_empty() {
        let key = var(&mut raw)?;
        let tag = usize::try_from(key >> 3).map_err(|_| "ACCOUNT_PROTO")?;
        if tag == 0 || tag >= types.len() || seen[tag] || key & 7 != u64::from(types[tag]) {
            return Err(Error::Invalid("ACCOUNT_PROTO"));
        }
        seen[tag] = true;
        match types[tag] {
            0 => out.nums[tag] = var(&mut raw)?,
            2 => {
                let len = usize::try_from(var(&mut raw)?).map_err(|_| "ACCOUNT_PROTO")?;
                if len > raw.len() {
                    return Err(Error::Invalid("ACCOUNT_PROTO"));
                }
                out.blobs[tag] = Some(&raw[..len]);
                raw = &raw[len..];
            }
            _ => return Err(Error::Invalid("ACCOUNT_PROTO")),
        }
    }
    Ok(out)
}
fn required<'a>(f: &Fields<'a>, tag: usize) -> Result<&'a [u8]> {
    f.blobs[tag]
        .filter(|b| !b.is_empty())
        .ok_or(Error::Invalid("ACCOUNT_PROTO"))
}
pub fn decode(snapshot: &Snapshot, owner: &[u8], raw: &[u8]) -> Result<Account> {
    let expected = address(owner)?;
    evidence::reference(raw, evidence::RPC)?;
    let v: Value = codec::unique_json(raw)?;
    let r = &v["result"]["response"];
    if snapshot.height() == 0
        || v["jsonrpc"] != "2.0"
        || v["id"] != 1
        || v.get("error").is_some()
        || (r["code"] != 0 && r["code"] != "0")
        || r["height"] != snapshot.height().to_string()
    {
        return Err(Error::Invalid("ACCOUNT_QUERY"));
    }
    let bytes = schema::bytes(&r["value"])?;
    if bytes.len() > 8192 {
        return Err(Error::Invalid("ACCOUNT_SIZE"));
    }
    let response = fields(&bytes, &[255, 2])?;
    let any = fields(required(&response, 1)?, &[255, 2, 2])?;
    if required(&any, 1)? != b"/cosmos.auth.v1beta1.BaseAccount" {
        return Err(Error::Invalid("ACCOUNT_TYPE"));
    }
    let base = fields(required(&any, 2)?, &[255, 2, 2, 0, 0])?;
    if required(&base, 1)? != expected.as_bytes() {
        return Err(Error::Invalid("ACCOUNT_OWNER"));
    }
    // The approved genesis installs ML-DSA keys. Refuse a missing/foreign key;
    // accepting an arbitrary address would decouple signer identity from auth.
    let pk_any = fields(required(&base, 2)?, &[255, 2, 2])?;
    if required(&pk_any, 1)? != b"/cosmos.crypto.mldsa65.PubKey" {
        return Err(Error::Invalid("ACCOUNT_KEY"));
    }
    let pk = fields(required(&pk_any, 2)?, &[255, 2])?;
    if codec::address(required(&pk, 1)?)?.as_slice() != owner {
        return Err(Error::Invalid("ACCOUNT_KEY"));
    }
    if let Some(a) = snapshot.value()["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| schema::bytes(&a["owner"]).is_ok_and(|b| b == owner))
    {
        if schema::num(&a["account_number"])? != base.nums[3]
            || schema::num(&a["sequence"])? != base.nums[4]
            || schema::bytes(&a["public_key"])? != required(&pk, 1)?
        {
            return Err(Error::Invalid("ACCOUNT_SNAPSHOT_CONFLICT"));
        }
    }
    Ok(Account {
        snapshot_id: snapshot.id().into(),
        owner: owner.to_vec(),
        number: base.nums[3],
        sequence: base.nums[4],
        raw: raw.to_vec(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use nus_exchange_contract::s3::{journal::canonical, snapshot::Binding};
    use serde_json::json;
    fn varout(mut n: u64) -> Vec<u8> {
        let mut out = vec![];
        loop {
            let b = (n & 127) as u8;
            n >>= 7;
            out.push(b | if n == 0 { 0 } else { 128 });
            if n == 0 {
                return out;
            }
        }
    }
    fn blob(tag: u8, raw: &[u8]) -> Vec<u8> {
        [vec![tag * 8 + 2], varout(raw.len() as u64), raw.to_vec()].concat()
    }
    fn uint(tag: u8, n: u64) -> Vec<u8> {
        [vec![tag * 8], varout(n)].concat()
    }
    fn snapshot() -> Snapshot {
        let f: Value = serde_json::from_str(include_str!(
            "../../../protocol/s3/vectors/correction-state-hash.json"
        ))
        .unwrap();
        let v = &f["initial_state"]["chain_snapshot"];
        let owners = v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| schema::bytes(&a["owner"]).unwrap())
            .collect();
        Binding::new(v["context"].clone(), owners, [4_000_000_000_000; 2], 0)
            .unwrap()
            .decode(&canonical(v).unwrap())
            .unwrap()
    }
    pub(crate) fn base(owner: &[u8], pk: &[u8], n: u64, seq: u64) -> Vec<u8> {
        [
            blob(1, address(owner).unwrap().as_bytes()),
            blob(
                2,
                &[
                    blob(1, b"/cosmos.crypto.mldsa65.PubKey"),
                    blob(2, &blob(1, pk)),
                ]
                .concat(),
            ),
            if n == 0 { vec![] } else { uint(3, n) },
            if seq == 0 { vec![] } else { uint(4, seq) },
        ]
        .concat()
    }
    pub(crate) fn rpc(s: &Snapshot, b: &[u8]) -> Vec<u8> {
        let response = blob(
            1,
            &[blob(1, b"/cosmos.auth.v1beta1.BaseAccount"), blob(2, b)].concat(),
        );
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":{"response":{"code":0,"height":s.height().to_string(),"value":STANDARD.encode(response)}}})).unwrap()
    }
    #[test]
    fn real_sdk_055_base_account_bytes_decode() {
        let s = snapshot();
        let fixture: Value =
            serde_json::from_str(include_str!("testdata/account-sdk.json")).unwrap();
        let owner = schema::bytes(&fixture["owner"]).unwrap();
        let raw = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":{"response":{
            "code":0,"height":s.height().to_string(),"value":fixture["account_response"]}}}))
        .unwrap();
        let a = decode(&s, &owner, &raw).unwrap();
        assert_eq!(a.at(&s, &owner).unwrap(), (9, 12));
        let request = request(&owner).unwrap();
        assert_eq!(request[0], 10);
        assert_eq!(
            codec::decode_address(std::str::from_utf8(&request[2..]).unwrap())
                .unwrap()
                .as_slice(),
            owner
        );
    }
    #[test]
    fn registered_account_matches_snapshot_and_raw_is_preserved() {
        let s = snapshot();
        let a = &s.value()["accounts"][0];
        let owner = schema::bytes(&a["owner"]).unwrap();
        let n = schema::num(&a["account_number"]).unwrap();
        let seq = schema::num(&a["sequence"]).unwrap();
        let raw = rpc(
            &s,
            &base(&owner, &schema::bytes(&a["public_key"]).unwrap(), n, seq),
        );
        let account = decode(&s, &owner, &raw).unwrap();
        assert_eq!(account.at(&s, &owner).unwrap(), (n, seq));
        assert_eq!(account.raw(), raw);
        assert!(account.at(&s, &[99; 20]).is_err());
        for (n, seq) in [(n + 1, seq), (n, seq + 1)] {
            assert!(
                decode(
                    &s,
                    &owner,
                    &rpc(
                        &s,
                        &base(&owner, &schema::bytes(&a["public_key"]).unwrap(), n, seq)
                    )
                )
                .is_err()
            );
        }
    }
    #[test]
    fn unregistered_auth_account_and_uint64_boundaries() {
        let s = snapshot();
        let pk = [42; 1952];
        let owner = codec::address(&pk).unwrap();
        for n in [0, 1, u64::MAX] {
            let raw = rpc(&s, &base(&owner, &pk, n, n));
            let a = decode(&s, &owner, &raw).unwrap();
            assert_eq!(a.at(&s, &owner).unwrap(), (n, n));
            let mut v = s.value().clone();
            v["height"] = json!((s.height() + 1).to_string());
            v.as_object_mut().unwrap().remove("snapshot_id");
            v["snapshot_id"] = json!(schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", &v).unwrap());
            let later = s.decode_related(&canonical(&v).unwrap()).unwrap();
            assert!(a.at(&later, &owner).is_err());
            assert!(decode(&later, &owner, &raw).is_err());
        }
    }
    #[test]
    fn address_key_type_and_owner_mismatch_rejected() {
        let s = snapshot();
        let pk = [42; 1952];
        let owner = codec::address(&pk).unwrap();
        assert!(decode(&s, &owner, &rpc(&s, &base(&[1; 20], &pk, 0, 0))).is_err());
        assert!(decode(&s, &owner, &rpc(&s, &base(&owner, &[43; 1952], 0, 0))).is_err());
        for key in [vec![], vec![0; 1951], vec![0; 1953]] {
            assert!(decode(&s, &owner, &rpc(&s, &base(&owner, &key, 0, 0))).is_err());
        }
        let mut bad = base(&owner, &pk, 0, 0);
        let i = bad.windows(7).position(|w| w == b"mldsa65").unwrap();
        bad[i] = b'X';
        assert!(decode(&s, &owner, &rpc(&s, &bad)).is_err());
        assert!(request(&[0; 19]).is_err());
        assert_eq!(
            request(&owner).unwrap(),
            blob(1, address(&owner).unwrap().as_bytes())
        );
    }
    #[test]
    fn protobuf_duplicates_unknown_truncation_overflow_nonminimal_rejected() {
        let s = snapshot();
        let pk = [42; 1952];
        let owner = codec::address(&pk).unwrap();
        let good = base(&owner, &pk, 1, 1);
        for suffix in [
            uint(3, 2),
            blob(1, b"duplicate"),
            blob(5, b"unknown"),
            vec![32, 128],
            vec![32, 128, 0],
            vec![32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2],
            vec![0],
            vec![42, 255, 127],
        ] {
            assert!(decode(&s, &owner, &rpc(&s, &[good.clone(), suffix].concat())).is_err());
        }
        for i in [0, 1, 2, 20, good.len() - 1] {
            assert!(decode(&s, &owner, &rpc(&s, &good[..i])).is_err());
        }
        assert!(fields(&[8, 128, 0], &[255, 0]).is_err());
        assert!(
            fields(
                &[10, 255, 255, 255, 255, 255, 255, 255, 255, 255, 1],
                &[255, 2]
            )
            .is_err()
        );
    }
    #[test]
    fn rpc_errors_and_wrong_wrapper_refused() {
        let s = snapshot();
        let pk = [42; 1952];
        let owner = codec::address(&pk).unwrap();
        let raw = rpc(&s, &base(&owner, &pk, 0, 0));
        for key in ["code", "height", "value"] {
            let mut v: Value = serde_json::from_slice(&raw).unwrap();
            v["result"]["response"][key] = json!("bad");
            assert!(decode(&s, &owner, &serde_json::to_vec(&v).unwrap()).is_err());
        }
        let mut v: Value = serde_json::from_slice(&raw).unwrap();
        v["error"] = Value::Null;
        assert!(decode(&s, &owner, &serde_json::to_vec(&v).unwrap()).is_err());
        assert!(decode(&s, &owner, br#"{"id":1,"id":1}"#).is_err());
        v["result"]["response"]["value"] = json!(STANDARD.encode(vec![0; 8193]));
        v.as_object_mut().unwrap().remove("error");
        assert!(decode(&s, &owner, &serde_json::to_vec(&v).unwrap()).is_err());
    }
}

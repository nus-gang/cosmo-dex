//! B's exact BatchV2 / Cosmos SIGN_MODE_DIRECT and ABCI JSON adapter.
//! Signing runs before store commit and outside the writer effect lock.
use super::*;
use crate::codec::{self, Codec};
use crate::s3::{
    evidence,
    journal::{canonical, sha256},
    snapshot::Snapshot,
    wire,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bech32::ToBase32;
use std::collections::BTreeSet;

/// Supplied by the local operator process. Never accept a signer via REST.
/// Implementations keep secret keys in memory and must not log sign documents.
pub trait OperatorSigner {
    fn public_key(&self) -> &[u8];
    fn sign(&self, document: &[u8]) -> Result<Vec<u8>>;
}
fn var(mut n: u64) -> Vec<u8> {
    let mut out = vec![];
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
    out
}
fn uint(tag: u64, n: u64) -> Vec<u8> {
    [var(tag << 3), var(n)].concat()
}
fn blob(tag: u64, b: &[u8]) -> Vec<u8> {
    [var(tag << 3 | 2), var(b.len() as u64), b.to_vec()].concat()
}

/// Reconstruct only the already-sealed C batch. Identity equality is mandatory;
/// this adapter never allocates batch IDs or chooses an alternative fill set.
pub fn sealed_batch(state: &Value, id: &str) -> Result<Vec<u8>> {
    let b = &state["batches"]
        .as_array()
        .ok_or("BATCH_NOT_FOUND")?
        .iter()
        .find(|b| b["batch"]["batch_id"] == id)
        .ok_or("BATCH_NOT_FOUND")?["batch"];
    let ids = b["fill_ids"].as_array().ok_or("BATCH_LIMIT")?;
    if ids.is_empty() || ids.len() > 8 {
        return Err(Error::Invalid("BATCH_LIMIT"));
    }
    let mut hashes = BTreeSet::new();
    let mut fills = vec![];
    for id in ids {
        let f = state["fills"]
            .as_array()
            .ok_or("BATCH_CONFLICT")?
            .iter()
            .find(|f| f["fill_id"] == *id)
            .ok_or("BATCH_CONFLICT")?;
        for k in ["buyer_order_hash", "seller_order_hash"] {
            hashes.insert(f[k].as_str().ok_or("BATCH_CONFLICT")?);
        }
        fills.push(json!({"fill_id":id,"maker_order_ref":f["maker_order_hash"],"taker_order_ref":f["taker_order_hash"],"buyer_order_ref":f["buyer_order_hash"],"seller_order_ref":f["seller_order_hash"],"quantity_lots":f["quantity_lots"],"execution_price_ticks":f["execution_price_ticks"],"fee_policy_version":f["fee_policy_version"],"command_seq":f["command_seq"],"match_index":f["match_index"]}));
    }
    let mut proofs = vec![];
    for hash in hashes {
        let o = state["orders"]
            .as_array()
            .ok_or("ORDER_NOT_FOUND")?
            .iter()
            .find(|o| o["view"]["order_hash"] == hash)
            .ok_or("ORDER_NOT_FOUND")?;
        proofs.push(json!({"order":Codec::default().decode("OrderV1",&schema::bytes(&o["order_wire"])?)?,"signature":o["signature"]}));
    }
    let c = &state["context"];
    let (raw, identity) = wire::seal(
        json!({"protocol_version":"2","chain_id":c["chain_id"],"genesis_hash":c["genesis_hash"],"exchange_module_id":"x/exchange","market_id":c["market_id"],"market_config_version":c["market_config_version"],"operator_epoch":b["operator_epoch"],"batch_seq":b["batch_seq"],"previous_batch_hash":b["previous_batch_hash"],"batch_id":b["batch_id"],"new_signed_orders":proofs,"fills":fills}),
    )?;
    if identity != *b {
        return Err(Error::Invalid("BATCH_CONFLICT"));
    }
    Ok(raw)
}
/// Account number/sequence come from authenticated B Account query at the same
/// observed chain; C verifies exact envelope policy and unresolved-attempt limits.
pub fn settle_attempt(
    snapshot: &Snapshot,
    batch: &[u8],
    attempt_no: u64,
    account_number: u64,
    sequence: u64,
    signer: &impl OperatorSigner,
) -> Result<(Value, Vec<u8>)> {
    let identity = wire::identity(batch)?;
    let pk = signer.public_key();
    if pk.len() != 1952 {
        return Err(Error::Invalid("KEY_LENGTH"));
    }
    let owner = hex::decode(sha256(pk)).map_err(|_| "KEY_LENGTH")?[..20].to_vec();
    if STANDARD.encode(&owner) != snapshot.value()["operator"] {
        return Err(Error::Invalid("FORBIDDEN"));
    }
    let op = bech32::encode("nus", owner.to_base32(), bech32::Variant::Bech32)
        .map_err(|_| "ADDRESS_MISMATCH")?;
    let timeout = snapshot.height().checked_add(8).ok_or("INTEGER_OVERFLOW")?;
    let msg = [blob(1, op.as_bytes()), blob(2, batch)].concat();
    let body = [
        blob(
            1,
            &[
                blob(1, b"/nus.exchange.s3.v1.MsgSettleBatch"),
                blob(2, &msg),
            ]
            .concat(),
        ),
        uint(3, timeout),
    ]
    .concat();
    let pkany = [
        blob(1, b"/cosmos.crypto.mldsa65.PubKey"),
        blob(2, &blob(1, pk)),
    ]
    .concat();
    let mut si = [blob(1, &pkany), blob(2, &blob(1, &uint(1, 1)))].concat();
    if sequence != 0 {
        si.extend(uint(3, sequence));
    }
    let fee = [
        blob(1, &[blob(1, b"DEVGAS"), blob(2, b"20000")].concat()),
        uint(2, 10_000_000),
    ]
    .concat();
    let auth = [blob(1, &si), blob(2, &fee)].concat();
    let mut doc = [
        blob(1, &body),
        blob(2, &auth),
        blob(
            3,
            snapshot.context()["chain_id"]
                .as_str()
                .ok_or("CONTEXT_MISMATCH")?
                .as_bytes(),
        ),
    ]
    .concat();
    if account_number != 0 {
        doc.extend(uint(4, account_number));
    }
    let signature = signer.sign(&doc)?;
    if !codec::verify_raw(pk, &doc, &signature, &[]) {
        return Err(Error::Invalid("INVALID_SIGNATURE"));
    }
    let tx = [blob(1, &body), blob(2, &auth), blob(3, &signature)].concat();
    let a = json!({"context":snapshot.context(),"batch":identity,"attempt_no":attempt_no.to_string(),"kind":"SETTLE","state":"PREPARED","operator":snapshot.value()["operator"],"operator_epoch":snapshot.value()["operator_epoch"],"account_number":account_number.to_string(),"account_sequence":sequence.to_string(),"timeout_height":timeout.to_string(),"first_possible_height":(snapshot.height()+1).to_string(),"gas_limit":"10000000","fee_atoms":"20000","raw_tx_ref":evidence::reference(&tx,evidence::TX)?,"tx_hash":sha256(&tx),"broadcast_count":"0","confirmed_tx":null,"absence_proof":null});
    schema::validate("Attempt", &a)?;
    Ok((a, tx))
}
/// S3 ABCI response value is canonical JSON, unlike S2's protobuf wrapper.
/// B/C validate context, height, snapshot hash, registered keys and conservation.
pub fn decode_snapshot(anchor: &Snapshot, raw_rpc: &[u8]) -> Result<Snapshot> {
    if raw_rpc.len() > evidence::limit(evidence::RPC)? {
        return Err(Error::Invalid("RPC_RESPONSE_LIMIT"));
    }
    let v = codec::unique_json(raw_rpc)?;
    if v["jsonrpc"] != "2.0" || v["id"] != 1 || v.get("error").is_some() {
        return Err(Error::Invalid("RPC_ERROR"));
    }
    let r = &v["result"]["response"];
    if r["code"] != 0 && r["code"] != "0" {
        return Err(Error::Invalid("RPC_ERROR"));
    }
    let raw = schema::bytes(&r["value"])?;
    let snap = anchor.decode_related(&raw)?;
    if r["height"] != snap.height().to_string() || canonical(snap.value())? != raw {
        return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
    }
    Ok(snap)
}

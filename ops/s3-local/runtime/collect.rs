//! Read-only collection through the reviewed L-D decoder and C proof verifier.
//! No browser input, engine command, freshness reset or automatic retry.
#[path = "query.rs"]
mod query;
use nus_exchange_contract::s3::{
    dev_local::{Error, Result},
    evidence::{Objects, RPC, TX},
    journal::{canonical, sha256},
    proof, schema,
    settlement_local::chain::decode_snapshot,
    snapshot::Snapshot,
};
pub use query::{Query, QueryRpc};
use serde_json::{Value, json};

pub struct ChainRead {
    rpc: QueryRpc,
}
impl ChainRead {
    pub fn new(addr: std::net::SocketAddr) -> Result<Self> {
        Ok(Self {
            rpc: QueryRpc::new(addr)?,
        })
    }
    /// Caller supplies its validated bootstrap/previous snapshot, never a
    /// browser-provided Context. Height zero means latest committed query;
    /// nonzero heights must match exactly. Caller still checks continuity.
    pub fn snapshot(&self, anchor: &Snapshot, height: u64) -> Result<(Snapshot, Vec<u8>)> {
        let data = canonical(&json!({"context":anchor.context(),"height":height.to_string()}))?;
        let raw = self.rpc.fetch(Query::Abci {
            path: "/nus.exchange.s3.v1.Query/Snapshot",
            data_hex: &hex::encode(data),
            height,
        })?;
        let snapshot = decode_snapshot(anchor, &raw)?;
        if height != 0 && snapshot.height() != height {
            return Err(Error::Invalid("SNAPSHOT_HEIGHT"));
        }
        Ok((snapshot, raw))
    }
    /// Inspect one already-validated snapshot height for an exact persisted TX.
    /// None means only "not in this block", never timeout/absence/failure proof.
    pub fn confirmed(&self, snapshot: &Snapshot, tx: &[u8]) -> Result<Option<(Value, Objects)>> {
        // Reject oversized/empty input before any network IO.
        nus_exchange_contract::s3::evidence::reference(tx, TX)?;
        let (block_ref, objects) = self.block(snapshot)?;
        confirmed_in_block(snapshot, tx, block_ref, objects)
    }
    /// Preserve both raw replies at the exact snapshot H and invoke C's
    /// existing chain-id/H/hash/transaction-count checks. This is a block
    /// observation only, not a terminal attempt or an absence proof.
    pub fn block(&self, snapshot: &Snapshot) -> Result<(Value, Objects)> {
        let block = self.rpc.fetch(Query::Block(snapshot.height()))?;
        let results = self.rpc.fetch(Query::BlockResults(snapshot.height()))?;
        checked_block(
            snapshot.context(),
            snapshot.height(),
            &snapshot.value()["block_hash"],
            &block,
            &results,
        )
    }
}
fn checked_block(
    context: &Value,
    height: u64,
    hash: &Value,
    block: &[u8],
    results: &[u8],
) -> Result<(Value, Objects)> {
    let mut objects = Objects::default();
    let br = objects.insert(block, RPC)?;
    let rr = objects.insert(results, RPC)?;
    let reference = json!({"height":height.to_string(),"block_hash":hash,"raw_block_response_ref":br,"raw_results_response_ref":rr});
    proof::block(&reference, context, &objects)?;
    Ok((reference, objects))
}
/// Assemble only inclusion metadata, then delegate all binding checks to C.
fn confirmed_in_block(
    snapshot: &Snapshot,
    tx: &[u8],
    mut reference: Value,
    mut objects: Objects,
) -> Result<Option<(Value, Objects)>> {
    let tx_ref = objects.insert(tx, TX)?;
    let (block, results) = proof::block(&reference, snapshot.context(), &objects)?;
    if reference["height"] != snapshot.height().to_string()
        || reference["block_hash"] != snapshot.value()["block_hash"]
    {
        return Err(Error::Invalid("PROOF_HISTORY_CONFLICT"));
    }
    let mut found = None;
    if let Some(txs) = block["block"]["data"]["txs"].as_array() {
        for (index, entry) in txs.iter().enumerate() {
            if schema::bytes(entry)? == tx {
                if found.replace(index).is_some() {
                    return Err(Error::Invalid("DUPLICATE_TX_IN_BLOCK"));
                }
            }
        }
    }
    let Some(index) = found else {
        return Ok(None);
    };
    let result = &results["txs_results"][index];
    reference["tx_hash"] = json!(sha256(tx));
    reference["raw_tx_ref"] = tx_ref;
    reference["tx_index"] = json!(index.to_string());
    for name in ["code", "gas_wanted", "gas_used"] {
        let n = match result[name].as_u64() {
            Some(n) => n,
            None => schema::num(&result[name])?,
        };
        reference[if name == "code" { "abci_code" } else { name }] = json!(n.to_string());
    }
    reference["codespace"] = result["codespace"].clone();
    proof::confirmed(&reference, snapshot.context(), &[snapshot], &objects)?;
    Ok(Some((reference, objects)))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn block() -> Vec<u8> {
        br#"{"result":{"block_id":{"hash":"AABB"},"block":{"header":{"chain_id":"nus-s3-local-demo","height":"9"},"data":{"txs":null}}}}"#.to_vec()
    }
    fn results() -> Vec<u8> {
        br#"{"result":{"height":"9","txs_results":null}}"#.to_vec()
    }
    fn check(b: &[u8], r: &[u8]) -> Result<(Value, Objects)> {
        checked_block(
            &json!({"chain_id":"nus-s3-local-demo"}),
            9,
            &json!("aabb"),
            b,
            r,
        )
    }
    #[test]
    fn raw_evidence_retained() {
        let (r, objects) = check(&block(), &results()).unwrap();
        assert_eq!(
            objects.resolve(&r["raw_block_response_ref"], RPC).unwrap(),
            block()
        );
        assert_eq!(
            objects
                .resolve(&r["raw_results_response_ref"], RPC)
                .unwrap(),
            results()
        );
    }
    #[test]
    fn mismatched_height_hash_chain_or_count_rejected() {
        for b in [
            String::from_utf8(block()).unwrap().replace("AABB", "CCDD"),
            String::from_utf8(block())
                .unwrap()
                .replace("\"9\"", "\"8\""),
            String::from_utf8(block())
                .unwrap()
                .replace("nus-s3-local-demo", "wrong-chain"),
            String::from_utf8(block())
                .unwrap()
                .replace("\"txs\":null", "\"txs\":[\"AA==\"]"),
        ] {
            assert!(check(b.as_bytes(), &results()).is_err());
        }
        assert!(check(&block(), br#"{"result":{"height":"8","txs_results":null}}"#).is_err());
    }
    #[test]
    fn duplicate_json_and_rpc_error_are_not_proofs() {
        assert!(check(br#"{"result":{},"result":{}}"#, &results()).is_err());
        assert!(check(&block(), br#"{"error":{"code":-32603}}"#).is_err());
    }
}

#[cfg(test)]
mod inclusion_tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use nus_exchange_contract::s3::snapshot::Binding;
    fn snapshot() -> Snapshot {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../protocol/s3/vectors/correction-state-hash.json"
        ))
        .unwrap();
        let v = &fixture["initial_state"]["chain_snapshot"];
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
    fn input(s: &Snapshot, txs: Vec<&[u8]>, code: Value) -> (Value, Objects) {
        let b = json!({"result":{"block_id":{"hash":s.value()["block_hash"]},
            "block":{"header":{"chain_id":s.context()["chain_id"],"height":s.height().to_string()},
            "data":{"txs":txs.iter().map(|t|STANDARD.encode(t)).collect::<Vec<_>>()}}}});
        let r = json!({"result":{"height":s.height().to_string(),"txs_results":
            txs.iter().map(|_|json!({"code":code,"codespace":"","gas_wanted":"10000000","gas_used":1234})).collect::<Vec<_>>()}});
        checked_block(
            s.context(),
            s.height(),
            &s.value()["block_hash"],
            &serde_json::to_vec(&b).unwrap(),
            &serde_json::to_vec(&r).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn exact_index_raw_and_numeric_fields_preserved() {
        let s = snapshot();
        let (r, o) = input(&s, vec![b"other", b"persisted tx"], json!(0));
        let (v, o) = confirmed_in_block(&s, b"persisted tx", r, o)
            .unwrap()
            .unwrap();
        assert_eq!(v["tx_index"], "1");
        assert_eq!(v["abci_code"], "0");
        assert_eq!(v["gas_used"], "1234");
        assert_eq!(o.resolve(&v["raw_tx_ref"], TX).unwrap(), b"persisted tx");
        proof::confirmed(&v, s.context(), &[&s], &o).unwrap();
        assert_eq!(o.entries().count(), 3);
    }
    #[test]
    fn nonzero_code_is_metadata_not_correction() {
        let s = snapshot();
        let (r, o) = input(&s, vec![b"tx"], json!(7));
        let (v, _) = confirmed_in_block(&s, b"tx", r, o).unwrap().unwrap();
        assert_eq!(v["abci_code"], "7");
        assert!(v.get("absence_proof").is_none());
        assert!(v.get("disposition").is_none());
    }
    #[test]
    fn absent_and_empty_blocks_return_no_proof() {
        let s = snapshot();
        for txs in [vec![], vec![b"other".as_slice()]] {
            let (r, o) = input(&s, txs, json!(0));
            assert!(confirmed_in_block(&s, b"tx", r, o).unwrap().is_none());
        }
    }
    #[test]
    fn duplicate_inclusion_rejected() {
        let s = snapshot();
        let (r, o) = input(&s, vec![b"tx", b"tx"], json!(0));
        assert!(confirmed_in_block(&s, b"tx", r, o).is_err());
    }
    #[test]
    fn malformed_numbers_rejected() {
        let s = snapshot();
        for code in [
            json!(-1),
            json!(1.5),
            json!("01"),
            json!("4294967296"),
            Value::Null,
        ] {
            let (r, o) = input(&s, vec![b"tx"], code);
            assert!(confirmed_in_block(&s, b"tx", r, o).is_err());
        }
    }
    #[test]
    fn snapshot_binding_and_raw_tampering_rejected() {
        let s = snapshot();
        let (mut r, o) = input(&s, vec![b"tx"], json!(0));
        r["block_hash"] = json!("00".repeat(32));
        assert!(confirmed_in_block(&s, b"tx", r, o).is_err());
        let (mut r, o) = input(&s, vec![b"tx"], json!(0));
        r["raw_results_response_ref"]["sha256"] = json!("00".repeat(32));
        assert!(confirmed_in_block(&s, b"tx", r, o).is_err());
    }
    #[test]
    fn invalid_tx_size_rejected() {
        let s = snapshot();
        for tx in [vec![], vec![0; 139265]] {
            let (r, o) = input(&s, vec![], json!(0));
            assert!(confirmed_in_block(&s, &tx, r, o).is_err());
        }
    }
}

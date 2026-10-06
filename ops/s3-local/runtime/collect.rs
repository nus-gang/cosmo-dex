//! Read-only collection through the reviewed L-D decoder and C proof verifier.
//! No browser input, engine command, freshness reset or automatic retry.
#[path = "query.rs"]
mod query;
use nus_exchange_contract::s3::{
    dev_local::{Error, Result},
    evidence::{Objects, RPC},
    journal::canonical,
    proof,
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

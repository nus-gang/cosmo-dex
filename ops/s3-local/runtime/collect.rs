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
    /// Raw Batch lookup at exactly the validated observation height.
    pub fn batch(&self, snapshot: &Snapshot, seq: u64) -> Result<(Value, Vec<u8>)> {
        if seq == 0 {
            return Err(Error::Invalid("BATCH_SEQUENCE"));
        }
        let data = canonical(&json!({"context":snapshot.context(),
            "height":snapshot.height().to_string(),"batch_seq":seq.to_string()}))?;
        let raw = self.rpc.fetch(Query::Abci {
            path: "/nus.exchange.s3.v1.Query/Batch",
            data_hex: &hex::encode(data),
            height: snapshot.height(),
        })?;
        Ok((decode_batch(snapshot, seq, &raw)?, raw))
    }
    /// Collect the whole eight-height timeout window, never infer absence from
    /// tx/NOT_FOUND. Inputs must be C's persisted attempt and snapshot history.
    /// At most 17 RPC calls; no mutation/broadcast/retry or freshness reset.
    pub fn absence(
        &self,
        current: &Snapshot,
        history: &[&Snapshot],
        attempt: &Value,
    ) -> Result<(Value, Objects)> {
        collect_absence(
            current,
            history,
            attempt,
            |seq| self.batch(current, seq).map(|(_, raw)| raw),
            |s| self.block(s),
        )
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
fn collect_absence(
    current: &Snapshot,
    history: &[&Snapshot],
    attempt: &Value,
    lookup: impl FnOnce(u64) -> Result<Vec<u8>>,
    mut fetch: impl FnMut(&Snapshot) -> Result<(Value, Objects)>,
) -> Result<(Value, Objects)> {
    schema::validate("Attempt", attempt)?;
    let first = schema::num(&attempt["first_possible_height"])?;
    let timeout = schema::num(&attempt["timeout_height"])?;
    if attempt["context"] != *current.context()
        || first == 0
        || timeout.checked_sub(first).and_then(|n| n.checked_add(1)) != Some(8)
        || current.height() <= timeout
        || history.len() != 8
    {
        return Err(Error::Invalid("ABSENCE_WINDOW"));
    }
    // Validate the complete bounded history before doing any IO. No arbitrary
    // height lookup or gap repair and no duplicate-height ambiguity.
    for (i, s) in history.iter().enumerate() {
        if s.context() != current.context() || s.height() != first + i as u64 {
            return Err(Error::Invalid("ABSENCE_HISTORY"));
        }
    }
    let account = current.value()["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["owner"] == attempt["operator"])
        .ok_or(Error::Invalid("ABSENCE_ACCOUNT"))?;
    if account["account_number"] != attempt["account_number"]
        || schema::num(&account["sequence"])? < schema::num(&attempt["account_sequence"])?
    {
        return Err(Error::Invalid("ABSENCE_ACCOUNT"));
    }
    let seq = schema::num(&attempt["batch"]["batch_seq"])?;
    let raw = lookup(seq)?;
    let batch = decode_batch(current, seq, &raw)?;
    if batch["status"] != "NOT_FOUND_AT_HEIGHT" {
        return Err(Error::Invalid("RECEIPT_PRESENT"));
    }
    let mut objects = Objects::default();
    objects.insert(&raw, RPC)?;
    let mut total = raw.len();
    let mut blocks = Vec::with_capacity(8);
    for s in history {
        let (reference, evidence) = fetch(s)?;
        // Keep one proof's raw evidence <=16MiB, independent of per-reply cap.
        // Only the two referenced RPC objects are accepted from the collector.
        for key in ["raw_block_response_ref", "raw_results_response_ref"] {
            let bytes = evidence.resolve(&reference[key], RPC)?;
            total = total
                .checked_add(bytes.len())
                .ok_or(Error::Invalid("EVIDENCE_SIZE"))?;
            if total > 16_777_216 {
                return Err(Error::Invalid("ABSENCE_EVIDENCE_LIMIT"));
            }
            objects.insert(bytes, RPC)?;
        }
        blocks.push(reference);
    }
    let p = json!({"tx_hash":attempt["tx_hash"],
        "first_possible_height":attempt["first_possible_height"],
        "timeout_height":attempt["timeout_height"],"observed_height":current.height().to_string(),
        "account_sequence":account["sequence"],"last_batch_seq":batch["last_seq"],
        "last_batch_hash":batch["last_hash"],"receipt_absent":true,
        "blocks":blocks,"observation_snapshot_id":current.id()});
    let mut candidate = attempt.clone();
    candidate["absence_proof"] = p.clone();
    proof::absence(&candidate, current, history, &objects)?;
    // No attempt state change here. Caller must persist via reviewed Worker/C.
    Ok((p, objects))
}
fn decode_batch(snapshot: &Snapshot, seq: u64, raw: &[u8]) -> Result<Value> {
    use nus_exchange_contract::codec;
    nus_exchange_contract::s3::evidence::reference(raw, RPC)?;
    let v = codec::unique_json(raw)?;
    if v["jsonrpc"] != "2.0" || v["id"] != 1 || v.get("error").is_some() {
        return Err(Error::Invalid("RPC_ERROR"));
    }
    let r = &v["result"]["response"];
    if (r["code"] != 0 && r["code"] != "0") || r["height"] != snapshot.height().to_string() {
        return Err(Error::Invalid("BATCH_QUERY_CONFLICT"));
    }
    let bytes = schema::bytes(&r["value"])?;
    let b = schema::decode("BatchLookup", &bytes)?;
    if seq == 0
        || canonical(&b)? != bytes
        || b["context"] != *snapshot.context()
        || b["observed_height"] != snapshot.height().to_string()
        || b["snapshot_id"] != snapshot.id()
        || b["requested_seq"] != seq.to_string()
        || b["last_seq"] != snapshot.value()["last_batch_seq"]
        || b["last_hash"] != snapshot.value()["last_batch_hash"]
    {
        return Err(Error::Invalid("BATCH_QUERY_CONFLICT"));
    }
    let last = schema::num(&b["last_seq"])?;
    if seq > last {
        if b["status"] != "NOT_FOUND_AT_HEIGHT" || !b["receipt"].is_null() {
            return Err(Error::Invalid("RECEIPT_INCONSISTENCY"));
        }
    } else if b["status"] != "FOUND" || b["receipt"].is_null() {
        return Err(Error::Invalid("RECEIPT_INCONSISTENCY"));
    }
    // FOUND is lookup data, not a verified ResolutionReceipt. Terminal TX and
    // Batch binding still belong to C's proof::receipt before any engine apply.
    Ok(b)
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

#[cfg(test)]
mod absence_tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use nus_exchange_contract::s3::snapshot::Binding;
    fn snapshot(h: u64) -> Snapshot {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../protocol/s3/vectors/correction-state-hash.json"
        ))
        .unwrap();
        let mut v = fixture["initial_state"]["chain_snapshot"].clone();
        let owners = v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| schema::bytes(&a["owner"]).unwrap())
            .collect();
        // Synthetic registered operator variant; real unregistered operators are
        // refused until a height-bound auth Account adapter supplies evidence.
        v["operator"] = v["accounts"][0]["owner"].clone();
        v["height"] = json!(h.to_string());
        v["block_hash"] = json!(sha256(h.to_string().as_bytes()));
        v.as_object_mut().unwrap().remove("snapshot_id");
        v["snapshot_id"] = json!(schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", &v).unwrap());
        Binding::new(v["context"].clone(), owners, [4_000_000_000_000; 2], 0)
            .unwrap()
            .decode(&canonical(&v).unwrap())
            .unwrap()
    }
    fn attempt(s: &Snapshot) -> Value {
        let a = s.value()["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["owner"] == s.value()["operator"])
            .unwrap();
        json!({"context":s.context(),"batch":{"batch_seq":"1","batch_id":"11".repeat(32),
            "batch_hash":"22".repeat(32),"previous_batch_hash":schema::ZERO,
            "operator_epoch":"1","fill_ids":["33".repeat(32)]},"attempt_no":"1",
            "kind":"SETTLE","state":"SUBMISSION_UNKNOWN","operator":a["owner"],
            "operator_epoch":"1","account_number":a["account_number"],"account_sequence":a["sequence"],
            "first_possible_height":"201","timeout_height":"208","gas_limit":"10000000",
            "fee_atoms":"20000","raw_tx_ref":nus_exchange_contract::s3::evidence::reference(b"tx",TX).unwrap(),
            "tx_hash":sha256(b"tx"),"broadcast_count":"1","confirmed_tx":null,"absence_proof":null})
    }
    fn lookup(s: &Snapshot) -> Value {
        json!({"context":s.context(),"observed_height":s.height().to_string(),"snapshot_id":s.id(),
            "requested_seq":"1","last_seq":s.value()["last_batch_seq"],
            "last_hash":s.value()["last_batch_hash"],"status":"NOT_FOUND_AT_HEIGHT","receipt":null})
    }
    fn rpc(s: &Snapshot, b: &Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":{"response":{
            "code":0,"height":s.height().to_string(),"value":STANDARD.encode(canonical(b).unwrap())}}})).unwrap()
    }
    fn block(s: &Snapshot, found: bool, broken: bool) -> Result<(Value, Objects)> {
        let b = json!({"result":{"block_id":{"hash":s.value()["block_hash"]},"block":{
            "header":{"chain_id":s.context()["chain_id"],"height":s.height().to_string(),
                "last_block_id":{"hash":if broken {schema::ZERO.to_string()} else {sha256((s.height()-1).to_string().as_bytes())}}},
            "data":{"txs":if found {json!([STANDARD.encode(b"tx")])} else {Value::Null}}}}});
        let r = json!({"result":{"height":s.height().to_string(),"txs_results":if found {json!([{}])} else {Value::Null}}});
        checked_block(
            s.context(),
            s.height(),
            &s.value()["block_hash"],
            &serde_json::to_vec(&b).unwrap(),
            &serde_json::to_vec(&r).unwrap(),
        )
    }
    #[test]
    fn full_window_raw_evidence_and_no_state_transition() {
        let s = snapshot(209);
        let a = attempt(&s);
        let original = a.clone();
        let history: Vec<_> = (201..=208).map(snapshot).collect();
        let mut calls = 0;
        let raw = rpc(&s, &lookup(&s));
        let (p, o) = collect_absence(
            &s,
            &history.iter().collect::<Vec<_>>(),
            &a,
            |_| Ok(raw.clone()),
            |s| {
                calls += 1;
                block(s, false, false)
            },
        )
        .unwrap();
        assert_eq!(calls, 8);
        assert_eq!(o.entries().count(), 17);
        assert_eq!(p["blocks"].as_array().unwrap().len(), 8);
        assert_eq!(a, original);
        assert!(p.get("state").is_none());
        assert!(o.entries().any(|(_, bytes)| bytes == raw));
        let mut checked = a;
        checked["absence_proof"] = p;
        proof::absence(&checked, &s, &history.iter().collect::<Vec<_>>(), &o).unwrap();
    }
    #[test]
    fn incomplete_window_timeout_or_account_rejected_before_io() {
        let current = snapshot(209);
        let history: Vec<_> = (201..=208).map(snapshot).collect();
        for case in 0..6 {
            let s = if case == 0 {
                snapshot(208)
            } else {
                current.clone()
            };
            let mut a = attempt(&s);
            let mut h = history.iter().collect::<Vec<_>>();
            match case {
                1 => {
                    h.pop();
                }
                2 => h.swap(0, 1),
                3 => a["timeout_height"] = json!("209"),
                4 => a["account_sequence"] = json!("999"),
                5 => a["account_number"] = json!("999"),
                _ => {}
            }
            assert!(
                collect_absence(
                    &s,
                    &h,
                    &a,
                    |_| panic!("lookup forbidden"),
                    |_| panic!("block forbidden")
                )
                .is_err()
            );
        }
    }
    #[test]
    fn included_tx_or_broken_chain_never_proves_absence() {
        let s = snapshot(209);
        let history: Vec<_> = (201..=208).map(snapshot).collect();
        for (found, broken) in [(true, false), (false, true)] {
            assert!(
                collect_absence(
                    &s,
                    &history.iter().collect::<Vec<_>>(),
                    &attempt(&s),
                    |_| Ok(rpc(&s, &lookup(&s))),
                    |h| block(h, found && h.height() == 204, broken && h.height() == 204)
                )
                .is_err()
            );
        }
    }
    #[test]
    fn unregistered_operator_refused_before_io() {
        let s = snapshot(209);
        let mut a = attempt(&s);
        a["operator"] = json!(STANDARD.encode([99; 20]));
        let history: Vec<_> = (201..=208).map(snapshot).collect();
        assert!(
            collect_absence(
                &s,
                &history.iter().collect::<Vec<_>>(),
                &a,
                |_| panic!("lookup forbidden"),
                |_| panic!("block forbidden")
            )
            .is_err()
        );
    }
    #[test]
    fn total_raw_evidence_budget_is_bounded() {
        let s = snapshot(209);
        let history: Vec<_> = (201..=208).map(snapshot).collect();
        let mut calls = 0;
        let result = collect_absence(
            &s,
            &history.iter().collect::<Vec<_>>(),
            &attempt(&s),
            |_| Ok(rpc(&s, &lookup(&s))),
            |s| {
                calls += 1;
                let (mut r, mut o) = block(s, false, false)?;
                let mut b: Value =
                    serde_json::from_slice(o.resolve(&r["raw_block_response_ref"], RPC)?).unwrap();
                b["padding"] = json!("x".repeat(2_097_152));
                r["raw_block_response_ref"] = o.insert(&serde_json::to_vec(&b).unwrap(), RPC)?;
                Ok((r, o))
            },
        );
        assert!(matches!(
            result,
            Err(Error::Invalid("ABSENCE_EVIDENCE_LIMIT"))
        ));
        assert_eq!(calls, 8);
    }
    #[test]
    fn rpc_failure_stops_without_retry_or_partial_proof() {
        let s = snapshot(209);
        let history: Vec<_> = (201..=208).map(snapshot).collect();
        let h = history.iter().collect::<Vec<_>>();
        assert!(
            collect_absence(
                &s,
                &h,
                &attempt(&s),
                |_| Err(Error::Invalid("RPC_ERROR")),
                |_| panic!("no blocks")
            )
            .is_err()
        );
        let mut calls = 0;
        assert!(
            collect_absence(
                &s,
                &h,
                &attempt(&s),
                |_| Ok(rpc(&s, &lookup(&s))),
                |s| {
                    calls += 1;
                    if calls == 3 {
                        Err(Error::Invalid("RPC_ERROR"))
                    } else {
                        block(s, false, false)
                    }
                }
            )
            .is_err()
        );
        assert_eq!(calls, 3);
    }
    #[test]
    fn lookup_context_height_id_sequence_last_and_status_binding() {
        let s = snapshot(209);
        for (key, value) in [
            ("observed_height", json!("208")),
            ("snapshot_id", json!(schema::ZERO)),
            ("requested_seq", json!("2")),
            ("last_seq", json!("1")),
            ("last_hash", json!("11".repeat(32))),
            ("status", json!("FOUND")),
            ("context", json!({})),
        ] {
            let mut b = lookup(&s);
            b[key] = value;
            assert!(decode_batch(&s, 1, &rpc(&s, &b)).is_err());
        }
        assert!(decode_batch(&s, 0, &rpc(&s, &lookup(&s))).is_err());
        let mut svalue = s.value().clone();
        svalue["last_batch_seq"] = json!("1");
        svalue["last_batch_hash"] = json!("11".repeat(32));
        svalue.as_object_mut().unwrap().remove("snapshot_id");
        svalue["snapshot_id"] = json!(schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", &svalue).unwrap());
        let later = s.decode_related(&canonical(&svalue).unwrap()).unwrap();
        assert!(decode_batch(&later, 1, &rpc(&later, &lookup(&later))).is_err());
    }
    #[test]
    fn malformed_or_noncanonical_rpc_not_promoted() {
        let s = snapshot(209);
        let valid = rpc(&s, &lookup(&s));
        for key in ["code", "height", "value"] {
            let mut v: Value = serde_json::from_slice(&valid).unwrap();
            v["result"]["response"][key] = json!("invalid");
            assert!(decode_batch(&s, 1, &serde_json::to_vec(&v).unwrap()).is_err());
        }
        let mut v: Value = serde_json::from_slice(&valid).unwrap();
        v["error"] = Value::Null;
        assert!(decode_batch(&s, 1, &serde_json::to_vec(&v).unwrap()).is_err());
        assert!(decode_batch(&s, 1, br#"{"id":1,"id":1}"#).is_err());
        let mut v: Value = serde_json::from_slice(&valid).unwrap();
        v["result"]["response"]["value"] =
            json!(STANDARD.encode(serde_json::to_vec_pretty(&lookup(&s)).unwrap()));
        assert!(decode_batch(&s, 1, &serde_json::to_vec(&v).unwrap()).is_err());
    }
}

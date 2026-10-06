//! Read-only collection through the reviewed L-D decoder and C proof verifier.
//! No browser input, engine command, freshness reset or automatic retry.
#[path = "account.rs"]
pub(crate) mod account;
#[path = "query.rs"]
mod query;
pub use account::Account;
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
    /// Query a chain-controlled account at exactly this trusted snapshot H.
    pub fn account(&self, snapshot: &Snapshot, owner: &[u8]) -> Result<Account> {
        let data = account::request(owner)?;
        if snapshot.height() == 0 {
            return Err(Error::Invalid("ACCOUNT_HEIGHT"));
        }
        let raw = self.rpc.fetch(Query::Abci {
            path: "/cosmos.auth.v1beta1.Query/Account",
            data_hex: &hex::encode(data),
            height: snapshot.height(),
        })?;
        account::decode(snapshot, owner, &raw)
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
    /// Assemble a COMMITTED receipt from the raw same-H Batch lookup and the
    /// exact persisted TX. The terminal snapshot must already be in C history.
    /// VOID needs separate resolution evidence and is rejected here.
    pub fn committed_receipt(
        &self,
        current: &Snapshot,
        terminal: &Snapshot,
        batch: &Value,
        tx: &[u8],
    ) -> Result<(Value, Objects)> {
        schema::validate("BatchIdentity", batch)?;
        nus_exchange_contract::s3::evidence::reference(tx, TX)?;
        if current.context() != terminal.context() || terminal.height() > current.height() {
            return Err(Error::Invalid("RECEIPT_HISTORY"));
        }
        let seq = schema::num(&batch["batch_seq"])?;
        let (_, raw) = self.batch(current, seq)?;
        collect_committed(current, terminal, batch, tx, &raw, || self.block(terminal))
    }
    /// Assemble VOID transport evidence using C's persisted failure evidence.
    /// This does not authorize correction: C record_receipt still checks the
    /// stored failure, CLOSE attempt, CLOSING state and all prior attempts.
    pub fn void_receipt(
        &self,
        current: &Snapshot,
        terminal: &Snapshot,
        batch: &Value,
        tx: &[u8],
        evidence_ref: &Value,
        persisted: &Objects,
    ) -> Result<(Value, Objects)> {
        validate_void_input(current, terminal, batch, tx, evidence_ref, persisted)?;
        let (_, raw) = self.batch(current, schema::num(&batch["batch_seq"])?)?;
        collect_void(
            current,
            terminal,
            batch,
            tx,
            evidence_ref,
            persisted,
            &raw,
            || self.block(terminal),
        )
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
            None,
            |seq| self.batch(current, seq).map(|(_, raw)| raw),
            |s| self.block(s),
        )
    }
    /// The Account has private fields and can only be constructed by the
    /// same-height decoder. Preserve its raw RPC in the returned evidence set.
    pub fn absence_with_account(
        &self,
        current: &Snapshot,
        history: &[&Snapshot],
        attempt: &Value,
        account: &Account,
    ) -> Result<(Value, Objects)> {
        collect_absence(
            current,
            history,
            attempt,
            Some(account),
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
fn collect_committed(
    current: &Snapshot,
    terminal: &Snapshot,
    batch: &Value,
    tx: &[u8],
    raw: &[u8],
    fetch: impl FnOnce() -> Result<(Value, Objects)>,
) -> Result<(Value, Objects)> {
    schema::validate("BatchIdentity", batch)?;
    nus_exchange_contract::s3::evidence::reference(tx, TX)?;
    if current.context() != terminal.context() || terminal.height() > current.height() {
        return Err(Error::Invalid("RECEIPT_HISTORY"));
    }
    let lookup = decode_batch(current, schema::num(&batch["batch_seq"])?, raw)?;
    let stored = &lookup["receipt"];
    if lookup["status"] != "FOUND"
        || stored["disposition"] != "COMMITTED"
        || stored["context"] != *current.context()
        || stored["batch"] != *batch
        || stored["terminal_height"] != terminal.height().to_string()
        || stored["terminal_tx_hash"] != sha256(tx)
        || !stored["failed_tx_hash"].is_null()
        || !stored["resolution_evidence_hash"].is_null()
    {
        return Err(Error::Invalid("RECEIPT_INCONSISTENCY"));
    }
    // Check lookup binding before block IO, then use the reviewed C proof.
    let (br, objects) = fetch()?;
    let (confirmed, mut objects) = confirmed_in_block(terminal, tx, br, objects)?
        .ok_or(Error::Invalid("RECEIPT_TX_MISSING"))?;
    objects.insert(raw, RPC)?;
    let receipt = json!({"context":current.context(),"batch":batch,
        "disposition":"COMMITTED","terminal_tx":confirmed,
        "batch_receipt_v2":stored["batch_receipt_v2"],
        "failed_tx_hash":null,"resolution_evidence_hash":null,"resolution_evidence_ref":null});
    proof::receipt(&receipt, batch, current, &[terminal], &objects)?;
    Ok((receipt, objects))
}
fn validate_void_input(
    current: &Snapshot,
    terminal: &Snapshot,
    batch: &Value,
    tx: &[u8],
    evidence_ref: &Value,
    persisted: &Objects,
) -> Result<(Value, Objects)> {
    schema::validate("BatchIdentity", batch)?;
    nus_exchange_contract::s3::evidence::reference(tx, TX)?;
    if current.context() != terminal.context() || terminal.height() > current.height() {
        return Err(Error::Invalid("RECEIPT_HISTORY"));
    }
    let evidence = persisted.typed(evidence_ref, "ResolutionEvidence")?;
    if evidence["context"] != *current.context() || evidence["batch"] != *batch {
        return Err(Error::Invalid("FAILURE_EVIDENCE_CONFLICT"));
    }
    // Copy only the referenced closure, not the entire runtime evidence store.
    // Re-resolve every byte under its original role; audit hash alone is useless.
    let mut objects = Objects::default();
    let root = json!({"resolution_evidence_ref":evidence_ref});
    let mut total = 0usize;
    for r in persisted.graph(&root)? {
        let media = r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?;
        let raw = persisted.resolve(&r, media)?;
        total = total.checked_add(raw.len()).ok_or("EVIDENCE_SIZE")?;
        if total > 16_777_216 {
            return Err(Error::Invalid("EVIDENCE_SIZE"));
        }
        objects.insert(raw, media)?;
    }
    Ok((evidence, objects))
}
fn collect_void(
    current: &Snapshot,
    terminal: &Snapshot,
    batch: &Value,
    tx: &[u8],
    evidence_ref: &Value,
    persisted: &Objects,
    raw: &[u8],
    fetch: impl FnOnce() -> Result<(Value, Objects)>,
) -> Result<(Value, Objects)> {
    let (evidence, mut objects) =
        validate_void_input(current, terminal, batch, tx, evidence_ref, persisted)?;
    let lookup = decode_batch(current, schema::num(&batch["batch_seq"])?, raw)?;
    let stored = &lookup["receipt"];
    if lookup["status"] != "FOUND"
        || stored["disposition"] != "VOID"
        || stored["context"] != *current.context()
        || stored["batch"] != *batch
        || stored["terminal_height"] != terminal.height().to_string()
        || stored["terminal_tx_hash"] != sha256(tx)
        || !stored["batch_receipt_v2"].is_null()
        || stored["failed_tx_hash"] != evidence["failed_tx_hash"]
        || stored["resolution_evidence_hash"]
            != schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", &evidence)?
    {
        return Err(Error::Invalid("RECEIPT_INCONSISTENCY"));
    }
    let (br, block_objects) = fetch()?;
    let (confirmed, block_objects) = confirmed_in_block(terminal, tx, br, block_objects)?
        .ok_or(Error::Invalid("RECEIPT_TX_MISSING"))?;
    for (r, bytes) in block_objects.entries() {
        objects.insert(bytes, r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?)?;
    }
    objects.insert(raw, RPC)?;
    let receipt = json!({"context":current.context(),"batch":batch,"disposition":"VOID",
        "terminal_tx":confirmed,"batch_receipt_v2":null,
        "failed_tx_hash":evidence["failed_tx_hash"],
        "resolution_evidence_hash":stored["resolution_evidence_hash"],
        "resolution_evidence_ref":evidence_ref});
    objects.graph(&receipt)?;
    proof::receipt(&receipt, batch, current, &[terminal], &objects)?;
    Ok((receipt, objects))
}
pub(super) fn collect_absence(
    current: &Snapshot,
    history: &[&Snapshot],
    attempt: &Value,
    auth: Option<&Account>,
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
    let (number, sequence) = if let Some(auth) = auth {
        auth.at(current, &schema::bytes(&attempt["operator"])?)?
    } else {
        let account = current.value()["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["owner"] == attempt["operator"])
            .ok_or(Error::Invalid("ABSENCE_ACCOUNT"))?;
        (
            schema::num(&account["account_number"])?,
            schema::num(&account["sequence"])?,
        )
    };
    if number != schema::num(&attempt["account_number"])?
        || sequence < schema::num(&attempt["account_sequence"])?
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
    if let Some(auth) = auth {
        objects.insert(auth.raw(), RPC)?;
        total += auth.raw().len();
    }
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
        "account_sequence":sequence.to_string(),"last_batch_seq":batch["last_seq"],
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
pub(crate) fn confirmed_in_block(
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
pub(crate) mod inclusion_tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use nus_exchange_contract::s3::snapshot::Binding;
    pub(super) fn snapshot() -> Snapshot {
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
    pub(crate) fn input(s: &Snapshot, txs: Vec<&[u8]>, code: Value) -> (Value, Objects) {
        let b = json!({"result":{"block_id":{"hash":s.value()["block_hash"]},
            "block":{"header":{"chain_id":s.context()["chain_id"],"height":s.height().to_string(),"last_block_id":{"hash":s.value()["block_hash"]}},
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
            None,
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
                    None,
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
                    None,
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
                None,
                |_| panic!("lookup forbidden"),
                |_| panic!("block forbidden")
            )
            .is_err()
        );
    }
    #[test]
    fn auth_account_enables_unregistered_operator_and_retains_raw() {
        let s = snapshot(209);
        let pk = [42; 1952];
        let owner = nus_exchange_contract::codec::address(&pk).unwrap();
        let raw = account::tests::rpc(&s, &account::tests::base(&owner, &pk, 9, 12));
        let auth = account::decode(&s, &owner, &raw).unwrap();
        let mut a = attempt(&s);
        a["operator"] = json!(STANDARD.encode(owner));
        a["account_number"] = json!("9");
        a["account_sequence"] = json!("12");
        let history: Vec<_> = (201..=208).map(snapshot).collect();
        let h = history.iter().collect::<Vec<_>>();
        let (p, o) = collect_absence(
            &s,
            &h,
            &a,
            Some(&auth),
            |_| Ok(rpc(&s, &lookup(&s))),
            |s| block(s, false, false),
        )
        .unwrap();
        assert_eq!(p["account_sequence"], "12");
        assert_eq!(o.entries().count(), 18);
        assert!(o.entries().any(|(_, bytes)| bytes == raw));
        for case in 0..4 {
            let mut bad = a.clone();
            let current = if case == 0 { snapshot(210) } else { s.clone() };
            match case {
                1 => bad["operator"] = json!(STANDARD.encode([99; 20])),
                2 => bad["account_number"] = json!("10"),
                3 => bad["account_sequence"] = json!("13"),
                _ => {}
            }
            assert!(
                collect_absence(
                    &current,
                    &h,
                    &bad,
                    Some(&auth),
                    |_| panic!("no lookup"),
                    |_| panic!("no block")
                )
                .is_err()
            );
        }
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
            None,
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
                None,
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
                None,
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

#[cfg(test)]
mod receipt_tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use nus_exchange_contract::{codec::Codec, s3::snapshot::Binding};
    fn setup() -> (Snapshot, Value, Value) {
        let old = super::inclusion_tests::snapshot();
        let mut v = old.value().clone();
        v["last_batch_seq"] = json!("1");
        v["last_batch_hash"] = json!("22".repeat(32));
        v["terminal_batch_seqs"] = json!(["1"]);
        v.as_object_mut().unwrap().remove("snapshot_id");
        v["snapshot_id"] = json!(schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", &v).unwrap());
        let owners = v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| schema::bytes(&a["owner"]).unwrap())
            .collect();
        let s = Binding::new(v["context"].clone(), owners, [4_000_000_000_000; 2], 0)
            .unwrap()
            .decode(&canonical(&v).unwrap())
            .unwrap();
        let batch = json!({"batch_seq":"1","batch_id":"11".repeat(32),
            "batch_hash":"22".repeat(32),"previous_batch_hash":schema::ZERO,
            "operator_epoch":"1","fill_ids":["33".repeat(32)]});
        let wire = json!({"protocol_version":"2","chain_id":s.context()["chain_id"],
            "genesis_hash":s.context()["genesis_hash"],"market_id":s.context()["market_id"],
            "batch_seq":"1","batch_id":batch["batch_id"],"batch_hash":batch["batch_hash"],
            "committed_height":s.height().to_string(),"tx_hash":sha256(b"tx")});
        let stored = json!({"context":s.context(),"batch":batch,"disposition":"COMMITTED",
            "terminal_height":s.height().to_string(),"terminal_tx_hash":sha256(b"tx"),
            "batch_receipt_v2":STANDARD.encode(Codec::default().encode("BatchReceiptV1", &wire).unwrap()),
            "failed_tx_hash":null,"resolution_evidence_hash":null});
        let lookup = json!({"context":s.context(),"observed_height":s.height().to_string(),
            "snapshot_id":s.id(),"requested_seq":"1","last_seq":"1",
            "last_hash":s.value()["last_batch_hash"],"status":"FOUND","receipt":stored});
        (s, batch, lookup)
    }
    fn raw(s: &Snapshot, lookup: &Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":{"response":{
            "code":0,"height":s.height().to_string(),"value":STANDARD.encode(canonical(lookup).unwrap())}}})).unwrap()
    }
    // Transport-only synthetic evidence. Empty attempts are intentionally not
    // an engine-authorized failure; successful collection cannot grant Apply.
    fn void_setup() -> (Snapshot, Value, Value, Value, Objects) {
        let (s, b, mut l) = setup();
        let e = json!({"context":s.context(),"batch":b,"observed_snapshot":s.value(),
            "settle_attempts":[],"batch_lookup":l,"failed_tx_hash":"44".repeat(32),
            "rejection_code":"UNEXPECTED_FINAL_REJECTION"});
        let mut o = Objects::default();
        let r = o.insert_typed("ResolutionEvidence", &e).unwrap();
        l["receipt"]["disposition"] = json!("VOID");
        l["receipt"]["batch_receipt_v2"] = Value::Null;
        l["receipt"]["failed_tx_hash"] = e["failed_tx_hash"].clone();
        l["receipt"]["resolution_evidence_hash"] =
            json!(schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", &e).unwrap());
        (s, b, l, r, o)
    }
    #[test]
    fn void_transport_preserves_exact_evidence_without_authorizing_apply() {
        let (s, b, l, r, o) = void_setup();
        let (receipt, out) = collect_void(&s, &s, &b, b"tx", &r, &o, &raw(&s, &l), || {
            Ok(super::inclusion_tests::input(&s, vec![b"tx"], json!(0)))
        })
        .unwrap();
        assert_eq!(receipt["disposition"], "VOID");
        assert_eq!(receipt["resolution_evidence_ref"], r);
        assert_eq!(
            out.typed(&r, "ResolutionEvidence").unwrap(),
            o.typed(&r, "ResolutionEvidence").unwrap()
        );
        assert_eq!(out.entries().count(), 5);
        out.graph(&receipt).unwrap();
    }
    #[test]
    fn void_audit_hash_without_original_evidence_refused_before_io() {
        let (s, b, l, r, _) = void_setup();
        assert!(
            collect_void(
                &s,
                &s,
                &b,
                b"tx",
                &r,
                &Objects::default(),
                &raw(&s, &l),
                || panic!("unexpected IO")
            )
            .is_err()
        );
    }
    #[test]
    fn void_lookup_mismatch_refused_before_block_io() {
        let (s, b, l, r, o) = void_setup();
        for (k, v) in [
            ("resolution_evidence_hash", json!(schema::ZERO)),
            ("failed_tx_hash", json!(schema::ZERO)),
            ("terminal_tx_hash", json!(schema::ZERO)),
            ("terminal_height", json!("999")),
            ("disposition", json!("COMMITTED")),
            ("batch_receipt_v2", json!("dHg=")),
        ] {
            let mut l = l.clone();
            l["receipt"][k] = v;
            assert!(
                collect_void(&s, &s, &b, b"tx", &r, &o, &raw(&s, &l), || panic!(
                    "unexpected IO"
                ))
                .is_err(),
                "{k}"
            );
        }
    }
    #[test]
    fn void_failed_or_missing_close_tx_is_not_receipt() {
        let (s, b, l, r, o) = void_setup();
        for (tx, code) in [
            (b"tx".as_slice(), json!(9)),
            (b"other".as_slice(), json!(0)),
        ] {
            assert!(
                collect_void(&s, &s, &b, b"tx", &r, &o, &raw(&s, &l), || Ok(
                    super::inclusion_tests::input(&s, vec![tx], code)
                ))
                .is_err()
            );
        }
    }
    #[test]
    fn void_evidence_identity_and_reference_tamper_refused() {
        let (s, b, l, r, o) = void_setup();
        let mut wrong = b.clone();
        wrong["batch_id"] = json!(schema::ZERO);
        assert!(
            collect_void(&s, &s, &wrong, b"tx", &r, &o, &raw(&s, &l), || panic!(
                "unexpected IO"
            ))
            .is_err()
        );
        let mut r = r;
        r["byte_length"] = json!("1");
        assert!(
            collect_void(&s, &s, &b, b"tx", &r, &o, &raw(&s, &l), || panic!(
                "unexpected IO"
            ))
            .is_err()
        );
    }
    #[test]
    fn committed_receipt_retains_lookup_tx_and_block_bytes() {
        let (s, b, l) = setup();
        let raw = raw(&s, &l);
        let (r, o) = collect_committed(&s, &s, &b, b"tx", &raw, || {
            Ok(super::inclusion_tests::input(
                &s,
                vec![b"other", b"tx"],
                json!(0),
            ))
        })
        .unwrap();
        proof::receipt(&r, &b, &s, &[&s], &o).unwrap();
        assert_eq!(r["terminal_tx"]["tx_index"], "1");
        assert_eq!(o.entries().count(), 4);
        let reference = nus_exchange_contract::s3::evidence::reference(&raw, RPC).unwrap();
        assert_eq!(o.resolve(&reference, RPC).unwrap(), raw);
    }
    #[test]
    fn receipt_identity_mismatch_refused_before_block_io() {
        let (s, b, l) = setup();
        for (k, v) in [
            ("terminal_height", json!("999")),
            ("terminal_tx_hash", json!(schema::ZERO)),
            ("batch", {
                let mut wrong = b.clone();
                wrong["batch_id"] = json!(schema::ZERO);
                wrong
            }),
            ("context", {
                let mut wrong = s.context().clone();
                wrong["genesis_hash"] = json!(schema::ZERO);
                wrong
            }),
        ] {
            let mut l = l.clone();
            l["receipt"][k] = v;
            assert!(
                collect_committed(&s, &s, &b, b"tx", &raw(&s, &l), || panic!("unexpected IO"))
                    .is_err()
            );
        }
    }
    #[test]
    fn void_and_missing_receipt_never_become_committed() {
        let (s, b, l) = setup();
        for void in [false, true] {
            let mut l = l.clone();
            if void {
                l["receipt"]["disposition"] = json!("VOID");
            } else {
                l["receipt"] = Value::Null;
                l["status"] = json!("NOT_FOUND_AT_HEIGHT");
            }
            assert!(
                collect_committed(&s, &s, &b, b"tx", &raw(&s, &l), || panic!("unexpected IO"))
                    .is_err()
            );
        }
    }
    #[test]
    fn failed_or_missing_terminal_tx_refused() {
        let (s, b, l) = setup();
        let raw = raw(&s, &l);
        for (tx, code) in [
            (b"tx".as_slice(), json!(7)),
            (b"other".as_slice(), json!(0)),
        ] {
            assert!(
                collect_committed(&s, &s, &b, b"tx", &raw, || Ok(
                    super::inclusion_tests::input(&s, vec![tx], code)
                ))
                .is_err()
            );
        }
    }
    #[test]
    fn forged_receipt_wire_refused_by_c_verifier() {
        let (s, b, mut l) = setup();
        l["receipt"]["batch_receipt_v2"] = json!(STANDARD.encode(b"forged"));
        assert!(
            collect_committed(&s, &s, &b, b"tx", &raw(&s, &l), || Ok(
                super::inclusion_tests::input(&s, vec![b"tx"], json!(0))
            ))
            .is_err()
        );
    }
    #[test]
    fn query_failure_or_block_failure_propagates() {
        let (s, b, l) = setup();
        assert!(
            collect_committed(
                &s,
                &s,
                &b,
                b"tx",
                br#"{"jsonrpc":"2.0","id":1,"error":{}}"#,
                || panic!("unexpected IO")
            )
            .is_err()
        );
        assert!(
            collect_committed(&s, &s, &b, b"tx", &raw(&s, &l), || Err(Error::Invalid(
                "RPC_UNAVAILABLE"
            )))
            .is_err()
        );
    }
}

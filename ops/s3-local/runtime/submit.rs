//! Trusted submission wiring. Never exposed through REST. C owns all transitions.
#[path = "collect.rs"]
mod collect;
#[path = "recovery.rs"]
mod recovery;
use collect::{Account, ChainRead};
use nus_exchange_contract::s3::{
    dev_local::{Command, Engine, Error, Result},
    evidence::{Objects, TYPED, reference},
    journal::canonical,
    schema,
    settlement_local::{LoopbackRpc, Worker, chain::OperatorSigner},
    snapshot::{Observation, Snapshot},
};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Prepared {
    pub tx_hash: String,
    /// Audit bytes only; not a chain receipt or a C terminal proof.
    pub account_rpc: Vec<u8>,
}
pub struct SubmitLane {
    engine: Arc<Engine>,
    worker: Worker,
    closed: bool,
}
fn now() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("CLOCK"))?
            .as_millis(),
    )
    .map_err(|_| Error::Invalid("CLOCK"))
}
impl SubmitLane {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            worker: Worker::new(engine.clone()),
            engine,
            closed: false,
        }
    }
    fn bound(&self, s: &Snapshot, o: &Observation, now: u64) -> Result<()> {
        let v = self.engine.reader().get()?;
        if v.gate == "RECOVERY_REQUIRED" {
            return Err(Error::Recovery("RECOVERY_REQUIRED"));
        }
        let latest = &v.state["latest_observation_ref"];
        let matches = if latest.is_null() {
            v.state["chain_snapshot"] == *s.value()
        } else {
            *latest == reference(&canonical(s.value())?, TYPED)?
        };
        if !matches || v.state["context"] != *s.context() {
            return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
        }
        s.freshness(o, now)?;
        Ok(())
    }
    /// Exactly one Account query and one prepare call; no broadcast or retry.
    /// Preserve the observer's timestamp, including time spent querying Account.
    pub fn prepare(
        &mut self,
        chain: &ChainRead,
        s: &Snapshot,
        batch: &str,
        attempt: u64,
        signer: &impl OperatorSigner,
        o: &Observation,
    ) -> Result<Prepared> {
        self.prepare_with(
            s,
            batch,
            attempt,
            signer,
            o,
            |s, owner| chain.account(s, owner),
            now,
        )
    }
    fn prepare_with(
        &mut self,
        s: &Snapshot,
        batch: &str,
        attempt: u64,
        signer: &impl OperatorSigner,
        o: &Observation,
        fetch: impl FnOnce(&Snapshot, &[u8]) -> Result<Account>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<Prepared> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true; // Error/unwind requires explicit restart from durable state.
        self.bound(s, o, clock()?)?;
        let owner = schema::bytes(&s.value()["operator"])?;
        let account = fetch(s, &owner)?;
        let (number, sequence) = account.at(s, &owner)?;
        let after_query = clock()?;
        self.bound(s, o, after_query)?;
        let hash = self.worker.prepare_settle(
            s,
            batch,
            attempt,
            number,
            sequence,
            signer,
            o,
            after_query,
        )?;
        self.closed = false;
        Ok(Prepared {
            tx_hash: hash,
            account_rpc: account.raw().to_vec(),
        })
    }
    /// Inspect one trusted observed height. Missing TX is not terminal evidence.
    /// No signing, broadcasting, receipt/Apply or correction occurs here.
    pub fn resolve_inclusion(
        &mut self,
        chain: &ChainRead,
        s: &Snapshot,
        hash: &str,
        o: &Observation,
    ) -> Result<bool> {
        self.resolve_inclusion_with(s, hash, o, |s, tx| chain.confirmed(s, tx), now)
    }
    fn resolve_inclusion_with(
        &mut self,
        s: &Snapshot,
        hash: &str,
        o: &Observation,
        fetch: impl FnOnce(&Snapshot, &[u8]) -> Result<Option<(serde_json::Value, Objects)>>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<bool> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        // Copy only immutable TX bytes under C's writer guard. No RPC/reentry in
        // this callback; this read must never be treated as broadcast permission.
        let (mut attempt, tx) = self
            .engine
            .with_committed_attempt(hash, |a, raw| (a.clone(), raw.to_vec()))?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        if attempt["context"] != *s.context()
            || s.height() < schema::num(&attempt["first_possible_height"])?
            || s.height() > schema::num(&attempt["timeout_height"])?
        {
            return Err(Error::Invalid("INCLUSION_HEIGHT"));
        }
        let found = fetch(s, &tx)?;
        self.bound(s, o, clock()?)?;
        let Some((confirmed, objects)) = found else {
            self.closed = false;
            return Ok(false);
        };
        // C validates full raw evidence/history and the immutable envelope.
        attempt["state"] = serde_json::json!(if confirmed["abci_code"] == "0" {
            "INCLUDED_SUCCESS"
        } else {
            "INCLUDED_FAILURE"
        });
        attempt["confirmed_tx"] = confirmed;
        let evidence = objects
            .entries()
            .map(|(r, raw)| {
                Ok((
                    raw.to_vec(),
                    r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?.into(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.worker
            .reconcile(Command::Resolve(attempt), &evidence, o, clock()?)?;
        self.closed = false;
        Ok(true)
    }
    /// Prove the complete timeout window through trusted RPC, then ask C to
    /// validate it against its own durable history. This never releases assets.
    pub fn resolve_absence(
        &mut self,
        chain: &ChainRead,
        s: &Snapshot,
        history: &[&Snapshot],
        hash: &str,
        o: &Observation,
    ) -> Result<()> {
        self.resolve_absence_with(
            s,
            hash,
            o,
            |s, a| {
                let owner = schema::bytes(&a["operator"])?;
                let account = chain.account(s, &owner)?;
                chain.absence_with_account(s, history, a, &account)
            },
            now,
        )
    }
    fn resolve_absence_with(
        &mut self,
        s: &Snapshot,
        hash: &str,
        o: &Observation,
        fetch: impl FnOnce(&Snapshot, &serde_json::Value) -> Result<(serde_json::Value, Objects)>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let mut attempt = self
            .engine
            .committed_attempt(hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        if attempt["context"] != *s.context()
            || s.height() <= schema::num(&attempt["timeout_height"])?
            || !matches!(
                attempt["state"].as_str(),
                Some("PREPARED" | "SUBMISSION_UNKNOWN")
            )
        {
            return Err(Error::Invalid("ABSENCE_POLICY"));
        }
        let (proof, objects) = fetch(s, &attempt)?;
        self.bound(s, o, clock()?)?;
        attempt["state"] = serde_json::json!("EXPIRED_ABSENT_PROVEN");
        attempt["absence_proof"] = proof;
        let evidence = objects
            .entries()
            .map(|(r, raw)| {
                Ok((
                    raw.to_vec(),
                    r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?.into(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.worker
            .reconcile(Command::Resolve(attempt), &evidence, o, clock()?)?;
        self.closed = false;
        Ok(())
    }
    /// Recover a successful SETTLE's exact TX/history from one C commit, then
    /// collect and persist its receipt. Applying balances remains a separate call.
    pub fn committed_receipt(
        &mut self, chain: &ChainRead, s: &Snapshot, index: usize, o: &Observation,
    ) -> Result<()> {
        self.committed_receipt_with(s, index, o,
            |s, terminal, batch, tx| chain.committed_receipt(s, terminal, batch, tx), now)
    }
    fn committed_receipt_with(
        &mut self, s: &Snapshot, index: usize, o: &Observation,
        fetch: impl FnOnce(&Snapshot, &Snapshot, &serde_json::Value, &[u8])
            -> Result<(serde_json::Value, Objects)>,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        self.bound(s, o, clock()?)?;
        let mut cursor = recovery::RecoveryCursor::open(self.engine.clone())?;
        let recovered = cursor.attempt_at(index)?.ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        let a = &recovered.attempt;
        if a["context"] != *s.context() || a["kind"] != "SETTLE"
            || a["state"] != "INCLUDED_SUCCESS" {
            return Err(Error::Invalid("RECEIPT_ATTEMPT"));
        }
        let h = schema::num(&a["confirmed_tx"]["height"])?;
        let page = cursor.history(Some(h), 1)?;
        let terminal = &page.observations.first().ok_or(Error::Invalid("PROOF_HISTORY_GAP"))?.snapshot;
        if terminal.height() != h || h > s.height() { return Err(Error::Invalid("RECEIPT_HISTORY")); }
        let tx = recovered.evidence.resolve(&a["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX)?;
        let (receipt, objects) = fetch(s, terminal, &a["batch"], tx)?;
        self.bound(s, o, clock()?)?;
        if self.engine.reader().get()?.commit != recovered.commit {
            return Err(Error::Invalid("STALE_COMMIT"));
        }
        if receipt["disposition"] != "COMMITTED" || receipt["batch"] != a["batch"]
            || receipt["terminal_tx"] != a["confirmed_tx"] {
            return Err(Error::Invalid("RECEIPT_INCONSISTENCY"));
        }
        let evidence = objects.entries().map(|(r, raw)| Ok((raw.to_vec(),
            r["media_type"].as_str().ok_or("EVIDENCE_TYPE")?.into())))
            .collect::<Result<Vec<_>>>()?;
        self.worker.reconcile(Command::Receipt(receipt), &evidence, o, clock()?)?;
        self.closed = false;
        Ok(())
    }
    /// Ask C to derive and validate final rejection from its persisted attempts
    /// and latest observation. This does not create CLOSE, VOID receipt or Apply.
    pub fn reject_final(&mut self, s: &Snapshot, o: &Observation) -> Result<()> {
        self.reject_final_with(s, o, now)
    }
    fn reject_final_with(
        &mut self, s: &Snapshot, o: &Observation,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<()> {
        if self.closed { return Err(Error::Recovery("SUBMIT_LANE_CLOSED")); }
        self.closed = true;
        let at = clock()?;
        self.bound(s, o, at)?;
        self.worker.reconcile(Command::RejectFinal, &[], o, at)?;
        self.closed = false;
        Ok(())
    }
    /// Apply only C's already persisted observations and terminal evidence.
    /// No RPC, signing, receipt synthesis or economic logic lives in this lane.
    pub fn apply(&mut self, s: &Snapshot, o: &Observation) -> Result<()> {
        self.apply_with(s, o, now)
    }
    fn apply_with(
        &mut self,
        s: &Snapshot,
        o: &Observation,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<()> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true;
        let at = clock()?;
        self.bound(s, o, at)?;
        self.worker.reconcile(Command::Apply, &[], o, at)?;
        self.closed = false;
        Ok(())
    }
    /// Only a persisted exact hash can be broadcast. C writes UNKNOWN/count
    /// before bounded IO under its writer gate. No signer or replacement here.
    pub fn broadcast_existing(
        &mut self,
        hash: &str,
        rpc: &LoopbackRpc,
        s: &Snapshot,
        o: &Observation,
    ) -> Result<serde_json::Value> {
        if self.closed {
            return Err(Error::Recovery("SUBMIT_LANE_CLOSED"));
        }
        self.closed = true;
        let time = now()?;
        self.bound(s, o, time)?;
        let a = self
            .engine
            .committed_attempt(hash)?
            .ok_or(Error::Invalid("ATTEMPT_NOT_FOUND"))?;
        if a["context"] != *s.context()
            || a["operator"] != s.value()["operator"]
            || a["operator_epoch"] != s.value()["operator_epoch"]
            || s.height() >= schema::num(&a["timeout_height"])?
        {
            return Err(Error::Invalid("ATTEMPT_POLICY"));
        }
        let result = self.worker.broadcast(hash, rpc, o, time)?;
        self.closed = false;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "../../../exchange/tests/support/dev_fixture.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, Signer},
    };
    use nus_exchange_contract::s3::{
        dev_local::{Command, Validated},
        snapshot::Binding,
    };
    use std::cell::Cell;
    struct Sign {
        pk: Vec<u8>,
        calls: Cell<usize>,
    }
    impl Sign {
        fn new() -> Self {
            Self {
                pk: hex::decode(fixture::key(16)["public_key_hex"].as_str().unwrap()).unwrap(),
                calls: Cell::new(0),
            }
        }
    }
    impl OperatorSigner for Sign {
        fn public_key(&self) -> &[u8] {
            &self.pk
        }
        fn sign(&self, doc: &[u8]) -> Result<Vec<u8>> {
            self.calls.set(self.calls.get() + 1);
            let seed: [u8; 32] = hex::decode(fixture::key(16)["test_seed_hex"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap();
            let (_, sk) = ml_dsa_65::KG::keygen_from_seed(&seed);
            Ok(sk.try_sign_with_seed(&[0; 32], doc, &[]).unwrap().to_vec())
        }
    }
    fn snapshot(v: &serde_json::Value, bps: u32) -> Snapshot {
        Binding::new(
            v["context"].clone(),
            v["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| schema::bytes(&a["owner"]).unwrap())
                .collect(),
            [4_000_000_000_000; 2],
            bps,
        )
        .unwrap()
        .decode(&canonical(v).unwrap())
        .unwrap()
    }
    fn account(s: &Snapshot, owner: &[u8]) -> Result<Account> {
        let pk = hex::decode(fixture::key(16)["public_key_hex"].as_str().unwrap()).unwrap();
        let raw =
            collect::account::tests::rpc(s, &collect::account::tests::base(owner, &pk, 16, 0));
        collect::account::decode(s, owner, &raw)
    }
    fn setup(
        bps: u32,
    ) -> (
        Arc<Engine>,
        Snapshot,
        String,
        nus_exchange_contract::s3::dev_local::Inputs,
        std::path::PathBuf,
    ) {
        let (inputs, v) = fixture::initial(bps);
        let home = fixture::home(bps);
        let e = Arc::new(
            Engine::create(
                &home,
                Validated::new(inputs.clone()).unwrap(),
                &canonical(&v).unwrap(),
            )
            .unwrap(),
        );
        let o = fixture::observation(&v);
        for (i, side, id) in [(0, "2", 241), (1, "1", 242)] {
            let (raw, sig) = fixture::sign_order(&v, i, side, 1000, 10000, id);
            e.execute(fixture::signed(&raw, &sig, i), &[], &o, fixture::NOW)
                .unwrap();
        }
        e.execute(Command::Seal("NORMAL".into()), &[], &o, fixture::NOW)
            .unwrap();
        let id = e.reader().get().unwrap().state["batches"][0]["batch"]["batch_id"]
            .as_str()
            .unwrap()
            .to_owned();
        (e, snapshot(&v, bps), id, inputs, home)
    }
    fn receipt_input(s: &Snapshot, terminal: &Snapshot, b: &serde_json::Value, tx: &[u8])
        -> Result<(serde_json::Value, Objects)> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let (confirmed, objects) = included(terminal, tx, 0)?.unwrap();
        let wire = serde_json::json!({"protocol_version":"2","chain_id":s.context()["chain_id"],
            "genesis_hash":s.context()["genesis_hash"],"market_id":s.context()["market_id"],
            "batch_seq":b["batch_seq"],"batch_id":b["batch_id"],"batch_hash":b["batch_hash"],
            "committed_height":terminal.height().to_string(),"tx_hash":confirmed["tx_hash"]});
        let bytes = nus_exchange_contract::codec::Codec::default().encode("BatchReceiptV1", &wire)?;
        Ok((serde_json::json!({"context":s.context(),"batch":b,"disposition":"COMMITTED",
            "terminal_tx":confirmed,"batch_receipt_v2":STANDARD.encode(bytes),
            "failed_tx_hash":null,"resolution_evidence_hash":null,"resolution_evidence_ref":null}), objects))
    }
    fn terminal_ready(bps: u32) -> (Arc<Engine>, SubmitLane, Snapshot,
        nus_exchange_contract::s3::dev_local::Inputs, std::path::PathBuf) {
        let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
        let a = e.committed_attempt(&hash).unwrap().unwrap();
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("102");
        v["last_batch_seq"] = a["batch"]["batch_seq"].clone();
        v["last_batch_hash"] = a["batch"]["batch_hash"].clone();
        v["terminal_batch_seqs"] = serde_json::json!([a["batch"]["batch_seq"]]);
        fixture::finish(&mut v);
        let s = snapshot(&v, bps);
        e.execute(Command::Snapshot(canonical(&v).unwrap()), &[], &fixture::observation(&v), fixture::NOW).unwrap();
        lane.resolve_inclusion_with(&s, &hash, &fixture::observation(&v),
            |s, tx| included(s, tx, 0), || Ok(fixture::NOW)).unwrap();
        (e, lane, s, inputs, home)
    }
    fn expected_failure(s: &Snapshot, tx: &[u8]) -> Result<Option<(serde_json::Value, Objects)>> {
        use nus_exchange_contract::s3::evidence::RPC;
        let (mut r, mut objects) = collect::inclusion_tests::input(s, vec![tx], serde_json::json!(1019));
        let mut raw: serde_json::Value = serde_json::from_slice(
            objects.resolve(&r["raw_results_response_ref"], RPC)?).unwrap();
        raw["result"]["txs_results"][0]["codespace"] = serde_json::json!("exchange_s3");
        r["raw_results_response_ref"] = objects.insert(&serde_json::to_vec(&raw).unwrap(), RPC)?;
        collect::confirmed_in_block(s, tx, r, objects)
    }
    #[test]
    fn final_rejection_persists_c_evidence_without_asset_release_and_replays() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
                |s, tx| expected_failure(s, tx), || Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap();
            lane.reject_final_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW)).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["batches"][0]["state"], "REJECTED_FINAL");
            // Failure bytes stay private and use the dedicated commit-pinned API.
            assert!(after.state.get("failure_evidence").is_none());
            let recovered = e.trusted_recovery_attempt_at(&after.commit, 0).unwrap().unwrap();
            assert!(recovered.evidence.entries().all(|(_, raw)|
                serde_json::from_slice::<serde_json::Value>(raw).ok()
                    .is_none_or(|v| v.get("rejection_code").is_none())));

            for k in ["accounts", "fills", "chain_snapshot", "corrections", "resolution_receipts", "attempt_refs"] {
                assert!(!before.state[k].is_null(), "missing {k}");
                assert_eq!(before.state[k], after.state[k], "{k}");
            }
            let batch = after.state["batches"][0]["batch"]["batch_id"].as_str().unwrap();
            let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
            let saved = cursor.failure(batch).unwrap().unwrap();
            assert_eq!(saved.commit, after.commit);
            assert_eq!(saved.raw, saved.evidence.resolve(&saved.resolution_evidence_ref, TYPED).unwrap());
            assert_eq!(saved.raw, canonical(&saved.resolution_evidence).unwrap());
            assert!(cursor.failure(&"00".repeat(32)).unwrap().is_none());
            drop(cursor); drop(lane); drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                assert_eq!(e.reader().get().unwrap().state, after.state);
                assert_eq!(e.reader().get().unwrap().commit, after.commit);
                let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
                let replay = cursor.failure(batch).unwrap().unwrap();
                assert_eq!(replay.raw, saved.raw);
                assert_eq!(replay.resolution_evidence_ref, saved.resolution_evidence_ref);
                assert_eq!(replay.evidence.entries().count(), saved.evidence.entries().count());
                assert!(e.with_committed_attempt(&hash, |_, _| panic!("terminal callback")).is_err());
            }
        }
    }
    #[test]
    fn final_rejection_refuses_unresolved_and_successful_attempts() {
        for success in [false, true] {
            let (e, lane, s, _, _, _) = prepared_at_next(0);
            let (e, mut lane, s) = if success {
                drop(lane); drop(e);
                let (e, lane, s, _, _) = terminal_ready(0); (e, lane, s)
            } else { (e, lane, s) };
            let before = e.reader().get().unwrap().commit.clone();
            assert!(lane.reject_final_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW)).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
            assert!(lane.reject_final_with(&s, &fixture::observation(s.value()), || panic!("closed clock")).is_err());
        }
    }
    #[test]
    fn final_rejection_stale_or_clock_failure_preserves_commit() {
        for stale in [false, true] {
            let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
            lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
                |s, tx| expected_failure(s, tx), || Ok(fixture::NOW)).unwrap();
            let before = e.reader().get().unwrap().commit.clone();
            assert!(lane.reject_final_with(&s, &fixture::observation(s.value()), ||
                if stale { Ok(fixture::NOW + 6000) } else { Err(Error::Invalid("CLOCK")) }).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
        }
    }
    #[test]
    fn receipt_persists_from_recovered_terminal_without_apply_and_replays() {
        for bps in [0, 25] {
            let (e, lane, s, inputs, home) = terminal_ready(bps);
            drop(lane); drop(e);
            let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
            let before = e.reader().get().unwrap();
            let mut lane = SubmitLane::new(e.clone());
            lane.committed_receipt_with(&s, 0, &fixture::observation(s.value()), receipt_input,
                || Ok(fixture::NOW)).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["resolution_receipts"].as_array().unwrap().len(), 1);
            for k in ["accounts", "fills", "chain_snapshot", "corrections"] {
                assert!(!before.state[k].is_null());
                assert_eq!(before.state[k], after.state[k], "{k}");
            }
            drop(lane); drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state, after.state);
                assert_eq!(e.reader().get().unwrap().commit, after.commit);
            }
        }
    }
    #[test]
    fn receipt_rejects_unresolved_attempt_before_io() {
        let (e, mut lane, s, _, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(lane.committed_receipt_with(&s, 0, &fixture::observation(s.value()),
            |_, _, _, _| panic!("unresolved IO"), || Ok(fixture::NOW)).is_err());
        assert!(lane.closed);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn receipt_forged_stale_or_io_failure_preserves_commit_and_closes() {
        for mode in 0..3 {
            let (e, mut lane, s, _, _) = terminal_ready(0);
            let before = e.reader().get().unwrap().commit.clone();
            let calls = Cell::new(0);
            assert!(lane.committed_receipt_with(&s, 0, &fixture::observation(s.value()),
                |s, t, b, tx| {
                    if mode == 2 { return Err(Error::Invalid("RPC_IO")); }
                    let (mut r, o) = receipt_input(s, t, b, tx)?;
                    if mode == 0 { r["batch_receipt_v2"] = serde_json::json!("dHg="); }
                    Ok((r, o))
                }, || { let n = calls.get(); calls.set(n+1);
                    Ok(fixture::NOW + if mode == 1 && n > 0 { 6000 } else { 0 }) }).is_err());
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
        }
    }
    #[test]
    fn apply_observation_preserves_unsettled_holds_and_replays() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            let before = e.reader().get().unwrap().state.clone();
            let attempt = e.committed_attempt(&hash).unwrap();
            lane.apply_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW))
                .unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.state["chain_snapshot"], *s.value());
            assert_eq!(e.committed_attempt(&hash).unwrap(), attempt);
            assert_eq!(
                after.state["resolution_receipts"],
                before["resolution_receipts"]
            );
            assert_eq!(after.state["corrections"], before["corrections"]);
            for key in ["accounts", "fills", "batches"] {
                assert!(!before[key].is_null(), "missing assertion field {key}");
                assert_eq!(after.state[key], before[key], "{key}");
            }
            let state = after.state.clone();
            let commit = after.commit.clone();
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let reopened =
                    Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(reopened.reader().get().unwrap().state, state);
                assert_eq!(reopened.reader().get().unwrap().commit, commit);
            }
        }
    }
    #[test]
    fn apply_stale_closes_lane_without_commit() {
        let (e, mut lane, s, _, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.apply_with(&s, &fixture::observation(s.value()), || Ok(
                fixture::NOW + 100_000
            ))
            .is_err()
        );
        assert!(
            lane.apply_with(&s, &fixture::observation(s.value()), || panic!(
                "closed lane called clock"
            ))
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn apply_wrong_anchor_and_clock_error_do_not_commit() {
        for fail_clock in [false, true] {
            let (e, mut lane, s, _, _, _) = prepared_at_next(0);
            let before = e.reader().get().unwrap().commit.clone();
            let mut v = s.value().clone();
            v["height"] = serde_json::json!("102");
            fixture::finish(&mut v);
            let wrong = snapshot(&v, 0);
            assert!(
                lane.apply_with(&wrong, &fixture::observation(wrong.value()), || {
                    if fail_clock {
                        Err(Error::Invalid("CLOCK"))
                    } else {
                        Ok(fixture::NOW)
                    }
                })
                .is_err()
            );
            assert!(lane.closed);
            assert_eq!(e.reader().get().unwrap().commit, before);
        }
    }
    #[test]
    fn fee_profiles_prepare_persists_exact_tx_and_two_replays() {
        for bps in [0, 25] {
            let (e, s, id, inputs, home) = setup(bps);
            let signer = Sign::new();
            let mut lane = SubmitLane::new(e.clone());
            let p = lane
                .prepare_with(
                    &s,
                    &id,
                    1,
                    &signer,
                    &fixture::observation(s.value()),
                    account,
                    || Ok(fixture::NOW),
                )
                .unwrap();
            assert_eq!(signer.calls.get(), 1);
            assert!(!p.account_rpc.is_empty());
            let a = e.committed_attempt(&p.tx_hash).unwrap().unwrap();
            assert_eq!(a["state"], "PREPARED");
            assert_eq!(a["broadcast_count"], "0");
            let state = e.reader().get().unwrap().state.clone();
            assert!(
                lane.prepare_with(
                    &s,
                    &id,
                    2,
                    &signer,
                    &fixture::observation(s.value()),
                    account,
                    || Ok(fixture::NOW)
                )
                .is_err()
            );
            assert_eq!(signer.calls.get(), 1); // Existing unresolved TX cannot be replaced.
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.reader().get().unwrap().state, state);
                assert_eq!(e.committed_attempt(&p.tx_hash).unwrap(), Some(a.clone()));
            }
        }
    }
    #[test]
    fn query_error_closes_lane_before_signing_and_retry_io() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e.clone());
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                |_, _| Err(Error::Invalid("RPC_IO")),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                |_, _| panic!("retry IO"),
                || panic!("retry clock")
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn stale_after_query_never_signs_or_commits() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e.clone());
        let before = e.reader().get().unwrap().commit.clone();
        let calls = Cell::new(0);
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                account,
                || {
                    let n = calls.get();
                    calls.set(n + 1);
                    Ok(fixture::NOW + if n == 0 { 0 } else { 6000 })
                }
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn different_account_snapshot_rejected_before_signing() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e);
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("101");
        fixture::finish(&mut v);
        let other = snapshot(&v, 0);
        assert!(
            lane.prepare_with(
                &s,
                &id,
                1,
                &sign,
                &fixture::observation(s.value()),
                |_, owner| account(&other, owner),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
    }
    #[test]
    fn unbound_snapshot_rejected_before_account_io() {
        let (e, s, id, _, _) = setup(0);
        let sign = Sign::new();
        let mut lane = SubmitLane::new(e);
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("101");
        fixture::finish(&mut v);
        let other = snapshot(&v, 0);
        assert!(
            lane.prepare_with(
                &other,
                &id,
                1,
                &sign,
                &fixture::observation(other.value()),
                |_, _| panic!("unbound IO"),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(sign.calls.get(), 0);
    }
    fn prepared_at_next(
        bps: u32,
    ) -> (
        Arc<Engine>,
        SubmitLane,
        Snapshot,
        String,
        nus_exchange_contract::s3::dev_local::Inputs,
        std::path::PathBuf,
    ) {
        let (e, s, id, inputs, home) = setup(bps);
        let mut lane = SubmitLane::new(e.clone());
        let p = lane
            .prepare_with(
                &s,
                &id,
                1,
                &Sign::new(),
                &fixture::observation(s.value()),
                account,
                || Ok(fixture::NOW),
            )
            .unwrap();
        let mut v = s.value().clone();
        v["height"] = serde_json::json!("101");
        fixture::finish(&mut v);
        let next = snapshot(&v, bps);
        e.execute(
            Command::Snapshot(canonical(&v).unwrap()),
            &[],
            &fixture::observation(&v),
            fixture::NOW,
        )
        .unwrap();
        (e, lane, next, p.tx_hash, inputs, home)
    }
    fn included(
        s: &Snapshot,
        tx: &[u8],
        code: u32,
    ) -> Result<Option<(serde_json::Value, Objects)>> {
        let (r, objects) = collect::inclusion_tests::input(s, vec![tx], serde_json::json!(code));
        collect::confirmed_in_block(s, tx, r, objects)
    }
    #[test]
    fn inclusion_persists_success_and_failure_without_releasing_assets_and_replays() {
        for bps in [0, 25] {
            for code in [0, 1019] {
                let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
                let before = e.reader().get().unwrap().state.clone();
                assert!(
                    lane.resolve_inclusion_with(
                        &s,
                        &hash,
                        &fixture::observation(s.value()),
                        |s, tx| included(s, tx, code),
                        || Ok(fixture::NOW)
                    )
                    .unwrap()
                );
                let a = e.committed_attempt(&hash).unwrap().unwrap();
                assert_eq!(
                    a["state"],
                    if code == 0 {
                        "INCLUDED_SUCCESS"
                    } else {
                        "INCLUDED_FAILURE"
                    }
                );
                assert_eq!(a["broadcast_count"], "0");
                let after = e.reader().get().unwrap().state.clone();
                // An inclusion alone does not confirm balances, correct fills,
                // apply a receipt, or replace the active batch.
                let mut before_assets = before.clone();
                let mut after_assets = after.clone();
                for key in ["attempt_refs", "last_command_seq", "stream_seq"] {
                    before_assets.as_object_mut().unwrap().remove(key);
                    after_assets.as_object_mut().unwrap().remove(key);
                }
                assert!(before_assets == after_assets, "non-attempt state changed");
                assert_ne!(before["attempt_refs"], after["attempt_refs"]);
                drop(lane);
                drop(e);
                for _ in 0..2 {
                    let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                    assert_eq!(e.committed_attempt(&hash).unwrap(), Some(a.clone()));
                    assert_eq!(e.reader().get().unwrap().state, after);
                }
            }
        }
    }
    /// Recover terminal raw and the unapplied anchor without broadcast authority.
    #[test]
    fn replay_terminal_raw_and_unapplied_observation_recover_privately() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home) = prepared_at_next(bps);
            lane.resolve_inclusion_with(
                &s, &hash, &fixture::observation(s.value()),
                |s, tx| included(s, tx, 0), || Ok(fixture::NOW),
            ).unwrap();
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let e = Arc::new(Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap());
                let view = e.reader().get().unwrap();
                assert_ne!(view.state["chain_snapshot"], *s.value());
                assert_eq!(view.state["latest_observation_ref"],
                    reference(&canonical(s.value()).unwrap(), TYPED).unwrap());
                assert_eq!(e.committed_attempt(&hash).unwrap().unwrap()["state"], "INCLUDED_SUCCESS");
                let called = Cell::new(false);
                let result = e.with_committed_attempt(&hash, |_, _| called.set(true));
                assert!(matches!(result, Err(Error::Invalid("ATTEMPT_TERMINAL"))));
                assert!(!called.get());
                let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
                assert_eq!(cursor.view().unwrap().commit, view.commit);
                assert_eq!(cursor.anchors().unwrap().latest.snapshot, s);
                assert_ne!(cursor.anchors().unwrap().applied.snapshot, s);
                let page = cursor.history(None, 64).unwrap();
                assert_eq!(page.observations.last().unwrap().snapshot, s);
                assert_eq!(page.next_height, None);
                let recovered = cursor.attempt_at(0).unwrap().unwrap();
                assert_eq!(recovered.commit, view.commit);
                assert_eq!(recovered.attempt["state"], "INCLUDED_SUCCESS");
                assert_eq!(recovered.attempt["tx_hash"], hash);
                let raw = recovered.evidence.resolve(&recovered.attempt["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX).unwrap();
                assert!(!raw.is_empty());
                assert!(cursor.attempt_at(1).unwrap().is_none());
                assert_eq!(e.reader().get().unwrap().state, view.state);
                assert_eq!(e.reader().get().unwrap().commit, view.commit);
            }
        }
    }
    #[test]
    fn recovery_cursor_stale_commit_closes_without_retry() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
        lane.resolve_inclusion_with(
            &s, &hash, &fixture::observation(s.value()),
            |s, tx| included(s, tx, 0), || Ok(fixture::NOW),
        ).unwrap();
        let before = e.reader().get().unwrap();
        assert!(matches!(cursor.attempt_at(0), Err(Error::Invalid("STALE_COMMIT"))));
        assert!(matches!(cursor.history(None, 1), Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"))));
        assert!(cursor.anchors().is_err());
        assert!(cursor.view().is_err());
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
        let mut fresh = recovery::RecoveryCursor::open(e.clone()).unwrap();
        assert_eq!(fresh.attempt_at(0).unwrap().unwrap().attempt["state"], "INCLUDED_SUCCESS");
    }
    #[test]
    fn failure_cursor_stale_commit_closes_without_reconstruction() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
        lane.resolve_inclusion_with(&s, &hash, &fixture::observation(s.value()),
            |s, tx| expected_failure(s, tx), || Ok(fixture::NOW)).unwrap();
        lane.reject_final_with(&s, &fixture::observation(s.value()), || Ok(fixture::NOW)).unwrap();
        let before = e.reader().get().unwrap();
        let batch = before.state["batches"][0]["batch"]["batch_id"].as_str().unwrap();
        assert!(matches!(cursor.failure(batch), Err(Error::Invalid("STALE_COMMIT"))));
        assert!(matches!(cursor.failure(batch), Err(Error::Recovery("RECOVERY_CURSOR_CLOSED"))));
        assert!(cursor.attempt_at(0).is_err());
        assert!(cursor.history(None, 1).is_err());
        assert!(cursor.view().is_err());
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
    }
    #[test]
    fn recovery_cursor_invalid_page_closes_and_preserves_store() {
        let (e, _, _, _, _, _) = prepared_at_next(25);
        let before = e.reader().get().unwrap();
        for limit in [0, 65, usize::MAX] {
            let mut cursor = recovery::RecoveryCursor::open(e.clone()).unwrap();
            assert!(cursor.history(None, limit).is_err());
            assert!(cursor.attempt_at(0).is_err());
            assert!(cursor.anchors().is_err());
        }
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
        assert_eq!(e.reader().get().unwrap().state, before.state);
    }
    #[test]
    fn inclusion_not_found_leaves_attempt_and_commit_unchanged() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let a = e.committed_attempt(&hash).unwrap();
        let before = e.reader().get().unwrap().commit.clone();
        for _ in 0..2 {
            assert!(
                !lane
                    .resolve_inclusion_with(
                        &s,
                        &hash,
                        &fixture::observation(s.value()),
                        |_, _| Ok(None),
                        || Ok(fixture::NOW)
                    )
                    .unwrap()
            );
        }
        assert_eq!(e.committed_attempt(&hash).unwrap(), a);
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    #[test]
    fn forged_inclusion_rejected_by_c_and_lane_stays_closed() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.resolve_inclusion_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |s, tx| {
                    let (mut v, o) = included(s, tx, 0)?.unwrap();
                    v["abci_code"] = serde_json::json!("1019");
                    Ok(Some((v, o)))
                },
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
        assert!(
            lane.resolve_inclusion_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |_, _| panic!("retry IO"),
                || panic!("retry clock")
            )
            .is_err()
        );
    }
    #[test]
    fn inclusion_stale_after_query_does_not_commit() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        let calls = Cell::new(0);
        assert!(
            lane.resolve_inclusion_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |s, tx| included(s, tx, 0),
                || {
                    let n = calls.get();
                    calls.set(n + 1);
                    Ok(fixture::NOW + if n == 0 { 0 } else { 6000 })
                }
            )
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
    fn expired(
        bps: u32,
    ) -> (
        Arc<Engine>,
        SubmitLane,
        Snapshot,
        String,
        nus_exchange_contract::s3::dev_local::Inputs,
        std::path::PathBuf,
        Vec<Snapshot>,
    ) {
        let (e, lane, first, hash, inputs, home) = prepared_at_next(bps);
        let timeout =
            schema::num(&e.committed_attempt(&hash).unwrap().unwrap()["timeout_height"]).unwrap();
        let mut history = vec![first.clone()];
        let mut current = first;
        for h in current.height() + 1..=timeout + 1 {
            let mut v = current.value().clone();
            v["height"] = serde_json::json!(h.to_string());
            fixture::finish(&mut v);
            current = snapshot(&v, bps);
            e.execute(
                Command::Snapshot(canonical(&v).unwrap()),
                &[],
                &fixture::observation(&v),
                fixture::NOW,
            )
            .unwrap();
            if h <= timeout {
                history.push(current.clone());
            }
        }
        (e, lane, current, hash, inputs, home, history)
    }
    fn absent(
        s: &Snapshot,
        a: &serde_json::Value,
        history: &[Snapshot],
    ) -> Result<(serde_json::Value, Objects)> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let b = serde_json::json!({"context":s.context(),"observed_height":s.height().to_string(),
            "snapshot_id":s.id(),"requested_seq":a["batch"]["batch_seq"],
            "last_seq":s.value()["last_batch_seq"],"last_hash":s.value()["last_batch_hash"],
            "status":"NOT_FOUND_AT_HEIGHT","receipt":null});
        let raw = serde_json::to_vec(
            &serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"response":{"code":0,
            "height":s.height().to_string(),"value":STANDARD.encode(canonical(&b)?)}}}),
        )
        .unwrap();
        let auth = account(s, &schema::bytes(&a["operator"])?)?;
        collect::collect_absence(
            s,
            &history.iter().collect::<Vec<_>>(),
            a,
            Some(&auth),
            |_| Ok(raw),
            |s| {
                Ok(collect::inclusion_tests::input(
                    s,
                    vec![],
                    serde_json::json!(0),
                ))
            },
        )
    }
    #[test]
    fn absence_persists_without_asset_release_and_replays_twice() {
        for bps in [0, 25] {
            let (e, mut lane, s, hash, inputs, home, history) = expired(bps);
            let before = e.reader().get().unwrap().state.clone();
            lane.resolve_absence_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |s, a| absent(s, a, &history),
                || Ok(fixture::NOW),
            )
            .unwrap();
            let a = e.committed_attempt(&hash).unwrap().unwrap();
            assert_eq!(a["state"], "EXPIRED_ABSENT_PROVEN");
            let after = e.reader().get().unwrap().state.clone();
            let mut left = before;
            let mut right = after.clone();
            for k in ["attempt_refs", "last_command_seq", "stream_seq"] {
                left.as_object_mut().unwrap().remove(k);
                right.as_object_mut().unwrap().remove(k);
            }
            assert!(left == right, "assets changed on absence");
            drop(lane);
            drop(e);
            for _ in 0..2 {
                let e = Engine::open(&home, Validated::new(inputs.clone()).unwrap()).unwrap();
                assert_eq!(e.committed_attempt(&hash).unwrap(), Some(a.clone()));
                assert_eq!(e.reader().get().unwrap().state, after);
            }
        }
    }
    #[test]
    fn absence_incomplete_forged_stale_or_io_error_never_commits() {
        for mode in 0..4 {
            let (e, mut lane, s, hash, _, _, history) = expired(0);
            let before = e.reader().get().unwrap().commit.clone();
            let calls = Cell::new(0);
            assert!(
                lane.resolve_absence_with(
                    &s,
                    &hash,
                    &fixture::observation(s.value()),
                    |s, a| {
                        if mode == 0 {
                            return absent(s, a, &history[..7]);
                        }
                        if mode == 3 {
                            return Err(Error::Invalid("RPC_IO"));
                        }
                        let (mut p, o) = absent(s, a, &history)?;
                        if mode == 1 {
                            p["tx_hash"] = serde_json::json!("00".repeat(32));
                        }
                        Ok((p, o))
                    },
                    || {
                        let n = calls.get();
                        calls.set(n + 1);
                        Ok(fixture::NOW + if mode == 2 && n > 0 { 6000 } else { 0 })
                    }
                )
                .is_err()
            );
            assert_eq!(e.reader().get().unwrap().commit, before);
            assert!(
                lane.resolve_absence_with(
                    &s,
                    &hash,
                    &fixture::observation(s.value()),
                    |_, _| panic!("retry IO"),
                    || panic!("retry clock")
                )
                .is_err()
            );
        }
    }
    #[test]
    fn absence_before_timeout_rejected_before_io() {
        let (e, mut lane, s, hash, _, _) = prepared_at_next(0);
        let before = e.reader().get().unwrap().commit.clone();
        assert!(
            lane.resolve_absence_with(
                &s,
                &hash,
                &fixture::observation(s.value()),
                |_, _| panic!("early IO"),
                || Ok(fixture::NOW)
            )
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().commit, before);
    }
}

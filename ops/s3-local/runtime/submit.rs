//! Trusted submission wiring. Never exposed through REST. C owns all transitions.
#[path = "collect.rs"]
mod collect;
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
}

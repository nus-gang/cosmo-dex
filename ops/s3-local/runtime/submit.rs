//! Trusted submission wiring. Never exposed through REST. C owns all transitions.
#[path = "collect.rs"]
mod collect;
use collect::{Account, ChainRead};
use nus_exchange_contract::s3::{
    dev_local::{Engine, Error, Result},
    evidence::{TYPED, reference},
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
}

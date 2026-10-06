//! Trusted sequential observation lane. No signing, submission, apply or repair.
#[path = "collect.rs"]
mod collect;
#[path = "recovery.rs"]
mod recovery;
use collect::ChainRead;
use nus_exchange_contract::s3::{
    dev_local::{Command, Error, Result},
    evidence::{RPC, TYPED, reference},
    journal::canonical,
    settlement_local::Worker,
    snapshot::{Observation, Snapshot},
};
use serde_json::Value;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct Observer {
    current: Snapshot,
    closed: bool,
}
impl Observer {
    /// Restore latest stored observation through C, independently of applied C.
    /// This does not issue freshness: the next tick must perform a trusted read.
    pub fn recover(engine: std::sync::Arc<nus_exchange_contract::s3::dev_local::Engine>) -> Result<Self> {
        let cursor = recovery::RecoveryCursor::open(engine)?;
        Self::new(cursor.anchors()?.latest.snapshot.clone(), &cursor.view()?.state)
    }

    /// Anchor must be decoded using the approved bootstrap binding. On restart
    /// pass the latest persisted observation, not merely the applied ledger H.
    pub fn new(current: Snapshot, state: &Value) -> Result<Self> {
        let latest = &state["latest_observation_ref"];
        let matches = if latest.is_null() {
            state["chain_snapshot"] == *current.value()
        } else {
            *latest == reference(&canonical(current.value())?, TYPED)?
        };
        if !matches || state["context"] != *current.context() {
            return Err(Error::Invalid("OBSERVER_ANCHOR"));
        }
        Ok(Self {
            current,
            closed: false,
        })
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.current
    }
    /// At most two bounded RPCs and one C commit per tick. Any error closes this
    /// lane permanently; restart requires reloading C's persisted state.
    pub fn tick(&mut self, chain: &ChainRead, worker: &Worker) -> Result<Observation> {
        let start = Instant::now();
        self.step(
            |anchor, height| chain.snapshot(anchor, height),
            || {
                let received_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Error::Invalid("CLOCK"))?
                    .as_millis();
                let latency = start.elapsed().as_millis();
                Ok((
                    u64::try_from(received_at).map_err(|_| Error::Invalid("CLOCK"))?,
                    u64::try_from(latency).map_err(|_| Error::Invalid("CLOCK"))?,
                ))
            },
            |next, raw, observation| commit_snapshot(worker, next, raw, observation),
        )
    }
    fn step(
        &mut self,
        mut fetch: impl FnMut(&Snapshot, u64) -> Result<(Snapshot, Vec<u8>)>,
        clock: impl FnOnce() -> Result<(u64, u64)>,
        commit: impl FnOnce(&Snapshot, &[u8], &Observation) -> Result<()>,
    ) -> Result<Observation> {
        if self.closed {
            return Err(Error::Recovery("OBSERVER_CLOSED"));
        }
        // Set before IO. An error or unwind cannot accidentally reuse old freshness.
        self.closed = true;
        let (latest, latest_raw) = fetch(&self.current, 0)?;
        if latest.context() != self.current.context() || latest.height() < self.current.height() {
            return Err(Error::Invalid("SNAPSHOT_CONFLICT"));
        }
        let next_height = self.current.height().checked_add(1);
        let catching_up =
            latest.height() > self.current.height() && next_height != Some(latest.height());
        let (next, raw) = if catching_up {
            let h = next_height.ok_or(Error::Invalid("INTEGER_OVERFLOW"))?;
            let pair = fetch(&self.current, h)?;
            if pair.0.height() != h {
                return Err(Error::Invalid("SNAPSHOT_HEIGHT"));
            }
            pair
        } else {
            (latest, latest_raw)
        };
        let changed = self.current.advance(&next)?;
        let (received_at, query_latency_ms) = clock()?;
        let observation = Observation {
            snapshot_id: next.id().into(),
            cursor_height: next.height(),
            received_at,
            query_latency_ms,
            catching_up,
        };
        // Stale/catching-up observations may be journaled for reconciliation.
        // Existing C/REST freshness checks still refuse admission; never reset time.
        if changed {
            commit(&next, &raw, &observation)?;
        }
        self.current = next;
        self.closed = false;
        Ok(observation)
    }
}

fn commit_snapshot(
    worker: &Worker,
    next: &Snapshot,
    raw: &[u8],
    observation: &Observation,
) -> Result<()> {
    worker.reconcile(
        Command::Snapshot(canonical(next.value())?),
        &[(raw.to_vec(), RPC.into())],
        observation,
        observation.received_at,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "../../../exchange/tests/support/dev_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use nus_exchange_contract::s3::{schema, snapshot::Binding};
    use serde_json::json;
    use std::cell::Cell;
    fn anchor() -> Snapshot {
        let v: Value = serde_json::from_str(include_str!(
            "../../../protocol/s3/vectors/correction-state-hash.json"
        ))
        .unwrap();
        let v = &v["initial_state"]["chain_snapshot"];
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
    fn at(s: &Snapshot, h: u64) -> Snapshot {
        let mut v = s.value().clone();
        v["height"] = json!(h.to_string());
        v.as_object_mut().unwrap().remove("snapshot_id");
        v["snapshot_id"] = json!(schema::hash("NUS/S3/CHAIN_SNAPSHOT/V1", &v).unwrap());
        s.decode_related(&canonical(&v).unwrap()).unwrap()
    }
    fn observer(s: Snapshot) -> Observer {
        let state =
            json!({"context":s.context(),"chain_snapshot":s.value(),"latest_observation_ref":null});
        Observer::new(s, &state).unwrap()
    }
    #[test]
    fn anchor_requires_persisted_latest() {
        let s = anchor();
        assert!(Observer::new(s.clone(), &json!({})).is_err());
        let state = json!({"context":s.context(),"latest_observation_ref":reference(&canonical(s.value()).unwrap(),TYPED).unwrap()});
        assert!(Observer::new(s.clone(), &state).is_ok());
        assert!(Observer::new(at(&s, s.height() + 1), &state).is_err());
    }
    #[test]
    fn unchanged_does_not_commit_or_reset_latency() {
        let s = anchor();
        let mut o = observer(s.clone());
        let result = o
            .step(
                |_, h| {
                    assert_eq!(h, 0);
                    Ok((s.clone(), vec![]))
                },
                || Ok((100, 2500)),
                |_, _, _| panic!("duplicate commit"),
            )
            .unwrap();
        assert_eq!(
            (
                result.received_at,
                result.query_latency_ms,
                result.catching_up
            ),
            (100, 2500, false)
        );
        assert!(s.freshness(&result, 100).is_err());
    }
    #[test]
    fn contiguous_commit_precedes_publication_and_preserves_raw() {
        let s = anchor();
        let n = at(&s, s.height() + 1);
        let mut o = observer(s);
        let calls = Cell::new(0);
        let result = o
            .step(
                |_, _| Ok((n.clone(), b"original RPC bytes".to_vec())),
                || Ok((100, 10)),
                |next, raw, obs| {
                    calls.set(calls.get() + 1);
                    assert_eq!(raw, b"original RPC bytes");
                    assert_eq!(obs.snapshot_id, next.id());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(o.snapshot(), &n);
        assert!(!result.catching_up);
    }
    #[test]
    fn gap_fetches_only_one_next_height_and_keeps_admission_closed() {
        let s = anchor();
        let mut o = observer(s.clone());
        let mut heights = vec![];
        let result = o
            .step(
                |a, h| {
                    heights.push(h);
                    Ok((at(a, if h == 0 { a.height() + 100 } else { h }), vec![]))
                },
                || Ok((100, 1)),
                |_, _, obs| {
                    assert!(obs.catching_up);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(heights, vec![0, s.height() + 1]);
        assert_eq!(o.snapshot().height(), s.height() + 1);
        assert!(result.catching_up);
    }
    #[test]
    fn commit_failure_keeps_anchor_and_permanently_closes_lane() {
        let s = anchor();
        let mut o = observer(s.clone());
        assert!(
            o.step(
                |a, _| Ok((at(a, a.height() + 1), vec![])),
                || Ok((100, 1)),
                |_, _, _| Err(Error::Recovery("IO"))
            )
            .is_err()
        );
        assert_eq!(o.snapshot(), &s);
        assert!(
            o.step(
                |_, _| panic!("retry"),
                || panic!("clock"),
                |_, _, _| panic!("commit")
            )
            .is_err()
        );
    }
    #[test]
    fn rollback_wrong_exact_height_and_fetch_error_never_commit() {
        for mode in 0..3 {
            let s = anchor();
            let mut o = observer(s.clone());
            assert!(
                o.step(
                    |a, h| match mode {
                        0 => Ok((at(a, a.height() - 1), vec![])),
                        1 => Ok((at(a, if h == 0 { a.height() + 3 } else { h + 1 }), vec![])),
                        _ => Err(Error::Invalid("QUERY_IO")),
                    },
                    || Ok((100, 1)),
                    |_, _, _| panic!("commit")
                )
                .is_err()
            );
            assert_eq!(o.snapshot(), &s);
            assert!(o.closed);
        }
    }
    #[test]
    fn real_worker_store_commit_and_replay_preserve_observation() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use nus_exchange_contract::s3::dev_local::{Engine, Validated};
        use nus_exchange_contract::s3::settlement_local::chain::decode_snapshot;
        use std::sync::Arc;
        for bps in [0, 25] {
            let (inputs, value) = super::fixture::initial(bps);
            let config = Validated::new(inputs.clone()).unwrap();
            let h = super::fixture::home(bps);
            let engine = Arc::new(Engine::create(&h, config, &canonical(&value).unwrap()).unwrap());
            let owners = value["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| schema::bytes(&a["owner"]).unwrap())
                .collect();
            let s = Binding::new(
                value["context"].clone(),
                owners,
                [4_000_000_000_000; 2],
                bps,
            )
            .unwrap()
            .decode(&canonical(&value).unwrap())
            .unwrap();
            let mut o = Observer::new(s.clone(), &engine.reader().get().unwrap().state).unwrap();
            let next = at(&s, s.height() + 1);
            let raw = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":{"response":{"code":0,"height":next.height().to_string(),"value":STANDARD.encode(canonical(next.value()).unwrap())}}})).unwrap();
            let worker = Worker::new(engine.clone());
            o.step(
                |a, _| Ok((decode_snapshot(a, &raw)?, raw.clone())),
                || Ok((super::fixture::NOW, 1)),
                |n, r, obs| commit_snapshot(&worker, n, r, obs),
            )
            .unwrap();
            let saved = engine.reader().get().unwrap();
            assert_eq!(saved.gate, "CATCHING_UP");
            assert_eq!(saved.state["chain_snapshot"], value); // C remains unapplied.
            assert_eq!(
                saved.state["latest_observation_ref"],
                reference(&canonical(next.value()).unwrap(), TYPED).unwrap()
            );
            assert!(Observer::new(s.clone(), &saved.state).is_err());
            drop(worker);
            drop(engine);
            for _ in 0..2 {
                let reopened = Arc::new(Engine::open(&h, Validated::new(inputs.clone()).unwrap()).unwrap());
                let replay = reopened.reader().get().unwrap();
                assert_eq!(replay.state, saved.state);
                assert_eq!(replay.commit, saved.commit);
                assert!(Observer::new(next.clone(), &replay.state).is_ok());
                assert_eq!(Observer::recover(reopened.clone()).unwrap().snapshot(), &next);
                assert_eq!(reopened.reader().get().unwrap().commit, replay.commit);
            }
        }
    }
}

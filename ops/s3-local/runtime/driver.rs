//! Trusted process wiring. Construction does not connect, bind, or start a service.
#[path = "observe.rs"]
mod observe;
#[path = "submit.rs"]
mod submit;
use nus_exchange_contract::s3::{
    dev_local::{Engine, Error, Result},
    settlement_local::{LoopbackRpc, Worker, chain::OperatorSigner},
    snapshot::{Observation, Snapshot},
};
use std::{net::SocketAddr, sync::Arc, time::{Duration, Instant}};

/// The service owner keeps the private signer and validated Engine alive.
/// Separate read clients have the same exact endpoint and no mutable cache.
pub struct Driver<S> {
    observer: observe::Observer,
    observe_chain: observe::collect::ChainRead,
    submit_chain: submit::collect::ChainRead,
    worker: Worker,
    submit: submit::SubmitLane,
    rpc: LoopbackRpc,
    signer: S,
    gate: TickGate,
    started: Instant,
}
impl<S: OperatorSigner> Driver<S> {
    pub fn recover(engine: Arc<Engine>, endpoint: SocketAddr, signer: S,
        lifetime: Duration, max_ticks: u64) -> Result<Self> {
        let gate = TickGate::new(lifetime, max_ticks)?;
        let observe_chain = observe::collect::ChainRead::new(endpoint)?;
        let submit_chain = submit::collect::ChainRead::new(endpoint)?;
        let rpc = LoopbackRpc::new(endpoint, Duration::from_secs(2))?;
        let observer = observe::Observer::recover(engine.clone())?;
        Ok(Self { observer, observe_chain, submit_chain, worker: Worker::new(engine.clone()),
            submit: submit::SubmitLane::new(engine), rpc, signer, gate, started: Instant::now() })
    }
    /// Called by the serialized REST lifecycle tick. At most one observation
    /// and one dispatcher action. No old observation is returned after error.
    pub fn tick(&mut self) -> Result<Observation> {
        self.gate.step(self.started.elapsed(), || {
            let o = self.observer.tick(&self.observe_chain, &self.worker)?;
            Ok((self.observer.snapshot().clone(), o))
        }, |s, o| {
            self.submit.reconcile_tick(&self.submit_chain, &self.rpc, s, &self.signer, o)?;
            Ok(())
        })
    }
    /// Trusted pair for the serialized HTTP lifecycle; never supplied by clients.
    pub fn tick_snapshot(&mut self) -> Result<(Snapshot, Observation)> {
        let observation = self.tick()?;
        Ok((self.observer.snapshot().clone(), observation))
    }
    pub fn stop(&mut self) { self.gate.closed = true; }
    /// Isolated fault child only. Consume the driver, observe once, then delegate
    /// one explicit Seal or Apply to the approved Worker. No dispatcher/sign/broadcast.
    #[cfg(feature = "fault-injection")]
    pub fn fault_storage_once(mut self, apply: bool, purpose: &str, point: &str, occurrence: u32, evidence: &std::path::Path,
        errno: Option<&str>, stop: &std::sync::atomic::AtomicBool) -> Result<(bool, bool)> {
        use std::sync::atomic::Ordering;
        if (apply && purpose != "NORMAL") || errno.is_some_and(|value| !["ENOSPC","EDQUOT","EIO"].contains(&value)) {
            return Err(Error::Invalid("STORAGE_FAULT_OPTIONS"));
        }
        if self.gate.closed || stop.load(Ordering::Relaxed) || self.started.elapsed() >= self.gate.lifetime {
            return Err(Error::Invalid("FAULT_DRIVER_CLOSED"));
        }
        self.gate.closed = true;
        let o = self.observer.tick(&self.observe_chain, &self.worker)?;
        if stop.load(Ordering::Relaxed) || self.started.elapsed() >= self.gate.lifetime || o.catching_up {
            return Err(Error::Invalid("FAULT_DRIVER_OBSERVATION"));
        }
        let (outcome, report) = if apply {
            self.submit.fault_apply_recorded(self.observer.snapshot(), &o, point, occurrence, errno, true, true, evidence)?
        } else { match errno {
            None => self.submit.fault_seal_recorded(self.observer.snapshot(),
                purpose, &o, point, occurrence, true, true, evidence)?,
            Some(value) => self.submit.fault_seal_errno_recorded(self.observer.snapshot(),
                purpose, &o, point, occurrence, value, true, true, evidence)?,
        }};
        Ok((outcome.is_ok(), report.injected))
    }
}

struct TickGate { lifetime: Duration, remaining: u64, last: Duration, closed: bool }
impl TickGate {
    fn new(lifetime: Duration, ticks: u64) -> Result<Self> {
        if lifetime.is_zero() || lifetime > Duration::from_secs(3600) || ticks == 0 || ticks > 3600 {
            return Err(Error::Invalid("DRIVER_LIMIT"));
        }
        Ok(Self { lifetime, remaining: ticks, last: Duration::ZERO, closed: false })
    }
    fn step(&mut self, elapsed: Duration,
        observe: impl FnOnce() -> Result<(Snapshot, Observation)>,
        work: impl FnOnce(&Snapshot, &Observation) -> Result<()>) -> Result<Observation> {
        if self.closed { return Err(Error::Recovery("DRIVER_CLOSED")); }
        self.closed = true;
        if elapsed < self.last { return Err(Error::Invalid("CLOCK_REGRESSION")); }
        if elapsed >= self.lifetime || self.remaining == 0 { return Err(Error::Invalid("DRIVER_LIMIT")); }
        self.last = elapsed;
        self.remaining -= 1;
        let (s, o) = observe()?;
        if s.id() != o.snapshot_id || s.height() != o.cursor_height {
            return Err(Error::Invalid("DRIVER_OBSERVATION"));
        }
        if !o.catching_up { work(&s, &o)?; }
        self.closed = false;
        Ok(o)
    }
}

#[cfg(test)]
#[path = "../../../exchange/tests/support/dev_fixture.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    fn pair(catchup: bool) -> (Snapshot, Observation) {
        let (_, v) = fixture::initial(0);
        let v = &v;
        let s = nus_exchange_contract::s3::snapshot::Binding::new(
            v["context"].clone(), v["accounts"].as_array().unwrap().iter()
                .map(|a| nus_exchange_contract::s3::schema::bytes(&a["owner"]).unwrap()).collect(),
            [4_000_000_000_000; 2], 0).unwrap()
            .decode(&nus_exchange_contract::s3::journal::canonical(v).unwrap()).unwrap();
        let o = Observation { snapshot_id: s.id().into(), cursor_height: s.height(),
            received_at: 1234, query_latency_ms: 87, catching_up: catchup };
        (s, o)
    }
    fn gate() -> TickGate { TickGate::new(Duration::from_secs(2), 2).unwrap() }
    #[test]
    fn driver_preserves_freshness_and_skips_catchup_work() {
        let mut g = gate();
        let o = g.step(Duration::ZERO, || Ok(pair(true)), |_, _| panic!("catchup effect")).unwrap();
        assert_eq!((o.received_at, o.query_latency_ms, o.catching_up), (1234,87,true));
        let calls = std::cell::Cell::new(0);
        let o = g.step(Duration::from_secs(1), || Ok(pair(false)), |_, o| {
            assert_eq!(o.received_at, 1234); calls.set(calls.get()+1); Ok(())
        }).unwrap();
        assert_eq!(calls.get(),1); assert_eq!(o.query_latency_ms,87);
        assert!(g.step(Duration::from_secs(1), || panic!(), |_,_| panic!()).is_err());
    }
    #[test]
    fn driver_errors_and_unwind_never_reuse_observation() {
        for kind in 0..4 {
            let mut g=gate();
            let (snapshot, observed) = pair(false);
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                g.step(Duration::ZERO, || {
                    if kind==0 { return Err(Error::Invalid("RPC")); }
                    let (s,mut o)=(snapshot, observed); if kind==1 { o.cursor_height+=1; } Ok((s,o))
                }, |_,_| if kind==3 { panic!("injected") } else { Err(Error::Invalid("WORK")) })
            }));
            if kind == 3 { assert!(result.is_err()); } else { assert!(result.unwrap().is_err()); }
            assert!(g.step(Duration::ZERO, || panic!(), |_,_| panic!()).is_err());
        }
    }
    #[test]
    fn driver_limits_and_clock_regression_precede_io() {
        for (secs,ticks) in [(0,1),(3601,1),(1,0),(1,3601)] {
            assert!(TickGate::new(Duration::from_secs(secs),ticks).is_err());
        }
        let mut g=gate();
        assert!(g.step(Duration::from_secs(2), || panic!(), |_,_| panic!()).is_err());
        let mut g=gate();
        g.step(Duration::from_secs(1), || Ok(pair(true)), |_,_| panic!()).unwrap();
        assert!(g.step(Duration::ZERO, || panic!(), |_,_| panic!()).is_err());
    }
}

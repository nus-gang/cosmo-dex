//! Bounded single-thread worker cadence. No threads, timers, signing or RPC here.
//! The process driver calls poll using monotonic milliseconds. A tick first
//! persists/refreshes the observation, then invokes at most one reconciliation
//! step. Catch-up observations never reach that effect callback.
use nus_exchange_contract::s3::{
    dev_local::{Error, Result},
    snapshot::{Observation, Snapshot},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    Waiting,
    CatchingUp,
    Advanced,
    Stopped,
}
pub struct Schedule {
    interval_ms: u64,
    deadline: u64,
    next: u64,
    last: u64,
    remaining: u64,
    closed: bool,
}
impl Schedule {
    pub fn new(start: u64, interval_ms: u64, lifetime_ms: u64, max_ticks: u64) -> Result<Self> {
        if interval_ms == 0 || lifetime_ms == 0 || max_ticks == 0 {
            return Err(Error::Invalid("SCHEDULE_BOUNDS"));
        }
        let deadline = start
            .checked_add(lifetime_ms)
            .ok_or(Error::Invalid("CLOCK"))?;
        Ok(Self {
            interval_ms,
            deadline,
            next: start,
            last: start,
            remaining: max_ticks,
            closed: false,
        })
    }
    pub fn stop(&mut self) {
        self.closed = true;
    }
    /// One call performs at most one observation and one work step, without
    /// retry/catch-up bursts. Callback error/unwind permanently closes the lane.
    pub fn poll(
        &mut self,
        at: u64,
        observe: impl FnOnce() -> Result<(Snapshot, Observation)>,
        work: impl FnOnce(&Snapshot, &Observation) -> Result<()>,
    ) -> Result<Progress> {
        if self.closed {
            return Ok(Progress::Stopped);
        }
        if at < self.last {
            self.closed = true;
            return Err(Error::Invalid("CLOCK_REGRESSION"));
        }
        self.last = at;
        if at >= self.deadline || self.remaining == 0 {
            self.closed = true;
            return Ok(Progress::Stopped);
        }
        if at < self.next {
            return Ok(Progress::Waiting);
        }
        self.closed = true;
        self.remaining -= 1;
        self.next = at
            .checked_add(self.interval_ms)
            .ok_or(Error::Invalid("CLOCK"))?;
        let (snapshot, observation) = observe()?;
        if snapshot.id() != observation.snapshot_id
            || snapshot.height() != observation.cursor_height
        {
            return Err(Error::Invalid("SCHEDULE_OBSERVATION"));
        }
        let result = if observation.catching_up {
            Progress::CatchingUp
        } else {
            work(&snapshot, &observation)?;
            Progress::Advanced
        };
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
    use std::cell::Cell;
    fn observed(catching_up: bool) -> (Snapshot, Observation) {
        let (_, v) = fixture::initial(0);
        let s = nus_exchange_contract::s3::snapshot::Binding::new(
            v["context"].clone(),
            v["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| nus_exchange_contract::s3::schema::bytes(&a["owner"]).unwrap())
                .collect(),
            [4_000_000_000_000; 2],
            0,
        )
        .unwrap()
        .decode(&nus_exchange_contract::s3::journal::canonical(&v).unwrap())
        .unwrap();
        let o = Observation {
            snapshot_id: s.id().into(),
            cursor_height: s.height(),
            received_at: 123,
            query_latency_ms: 7,
            catching_up,
        };
        (s, o)
    }
    fn no_observe() -> Result<(Snapshot, Observation)> {
        panic!("unexpected observation")
    }
    fn no_work(_: &Snapshot, _: &Observation) -> Result<()> {
        panic!("unexpected work")
    }
    #[test]
    fn bounded_cadence_no_burst_or_timestamp_rewrite() {
        let mut lane = Schedule::new(10, 20, 1000, 2).unwrap();
        let calls = Cell::new(0);
        assert_eq!(
            lane.poll(
                10,
                || Ok(observed(false)),
                |_, o| {
                    assert_eq!(o.received_at, 123);
                    assert_eq!(o.query_latency_ms, 7);
                    calls.set(calls.get() + 1);
                    Ok(())
                }
            )
            .unwrap(),
            Progress::Advanced
        );
        assert_eq!(
            lane.poll(29, no_observe, no_work).unwrap(),
            Progress::Waiting
        );
        assert_eq!(
            lane.poll(
                500,
                || Ok(observed(false)),
                |_, _| {
                    calls.set(calls.get() + 1);
                    Ok(())
                }
            )
            .unwrap(),
            Progress::Advanced
        );
        assert_eq!(
            lane.poll(500, no_observe, no_work).unwrap(),
            Progress::Stopped
        );
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn catchup_persists_without_effect() {
        let mut lane = Schedule::new(0, 1, 100, 2).unwrap();
        assert_eq!(
            lane.poll(0, || Ok(observed(true)), no_work).unwrap(),
            Progress::CatchingUp
        );
        assert_eq!(
            lane.poll(1, || Ok(observed(false)), |_, _| Ok(())).unwrap(),
            Progress::Advanced
        );
    }
    #[test]
    fn observation_and_work_errors_close_without_retry() {
        for observe_fails in [true, false] {
            let mut lane = Schedule::new(0, 1, 100, 2).unwrap();
            assert!(
                lane.poll(
                    0,
                    || if observe_fails {
                        Err(Error::Invalid("IO"))
                    } else {
                        Ok(observed(false))
                    },
                    |_, _| Err(Error::Invalid("WORK"))
                )
                .is_err()
            );
            assert_eq!(
                lane.poll(1, no_observe, no_work).unwrap(),
                Progress::Stopped
            );
        }
    }
    #[test]
    fn mismatch_and_clock_regression_close() {
        let mut lane = Schedule::new(5, 1, 100, 2).unwrap();
        assert!(lane.poll(4, no_observe, no_work).is_err());
        assert_eq!(
            lane.poll(5, no_observe, no_work).unwrap(),
            Progress::Stopped
        );
        let mut lane = Schedule::new(0, 1, 100, 2).unwrap();
        assert!(
            lane.poll(
                0,
                || {
                    let (s, mut o) = observed(false);
                    o.cursor_height += 1;
                    Ok((s, o))
                },
                no_work
            )
            .is_err()
        );
        assert_eq!(
            lane.poll(1, no_observe, no_work).unwrap(),
            Progress::Stopped
        );
    }
    #[test]
    fn unwinding_callback_leaves_schedule_closed() {
        let mut lane = Schedule::new(0, 1, 100, 2).unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = lane.poll(0, || Ok(observed(false)), |_, _| panic!("injected"));
            }))
            .is_err()
        );
        assert_eq!(
            lane.poll(1, no_observe, no_work).unwrap(),
            Progress::Stopped
        );
    }
    #[test]
    fn stop_deadline_and_invalid_bounds_do_no_io() {
        for (i, l, n) in [(0, 1, 1), (1, 0, 1), (1, 1, 0)] {
            assert!(Schedule::new(0, i, l, n).is_err());
        }
        assert!(Schedule::new(u64::MAX, 1, 1, 1).is_err());
        let mut lane = Schedule::new(0, 1, 10, 10).unwrap();
        assert_eq!(
            lane.poll(10, no_observe, no_work).unwrap(),
            Progress::Stopped
        );
        let mut lane = Schedule::new(0, 1, 10, 10).unwrap();
        lane.stop();
        assert_eq!(
            lane.poll(0, no_observe, no_work).unwrap(),
            Progress::Stopped
        );
    }
}

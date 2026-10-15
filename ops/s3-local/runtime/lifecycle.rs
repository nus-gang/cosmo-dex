//! Serialized bounded service loop. Construction/approval and trusted worker
//! semantics belong to the caller. This module never signs or changes C state.
use std::{
    io,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(25);
const TICK: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
pub struct Limits {
    pub lifetime: Duration,
    pub requests: u64,
}
impl Limits {
    fn validate(self) -> Result<Self, &'static str> {
        if self.lifetime.is_zero()
            || self.lifetime > Duration::from_secs(3600)
            || self.requests == 0
            || self.requests > 100_000
        {
            return Err("RESOURCE_LIMIT");
        }
        Ok(self)
    }
}
#[derive(Debug, PartialEq)]
pub enum Stop {
    Requested,
    Lifetime,
    RequestLimit,
}
#[derive(Debug, PartialEq)]
pub struct Report {
    pub reason: Stop,
    pub handled: u64,
    pub rejected: u64,
    pub ticks: u64,
}

/// A pre-existing, already-bound listener must match the reviewed endpoint.
/// No address fallback, automatic port selection, or bind occurs here.
pub fn endpoint(addr: SocketAddr) -> Result<(), &'static str> {
    if addr.ip() != Ipv4Addr::LOCALHOST || addr.port() < 1024 {
        return Err("LOOPBACK_ENDPOINT_REQUIRED");
    }
    Ok(())
}

/// The caller retains Engine/Rest and supplies a trusted bounded worker tick.
/// There is one in-flight connection and no thread/request queue. Every tick
/// completes before accepting a request. Any tick failure ends the service;
/// it is not converted to a fresh Observation or retried by this loop.
pub fn serve(
    listener: TcpListener,
    expected: SocketAddr,
    stop: &AtomicBool,
    limits: Limits,
    tick: impl FnMut() -> Result<(), &'static str>,
    handle: impl FnMut(TcpStream) -> Result<(), &'static str>,
) -> Result<Report, &'static str> {
    endpoint(expected)?;
    limits.validate()?;
    if listener.local_addr().map_err(|_| "LISTENER_ADDRESS")? != expected {
        return Err("LISTENER_MISMATCH");
    }
    listener
        .set_nonblocking(true)
        .map_err(|_| "LISTENER_MODE")?;
    let start = Instant::now();
    drive(
        stop,
        limits,
        || start.elapsed(),
        std::thread::sleep,
        || match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    return Err("PEER_REJECTED");
                }
                // Accepted sockets need explicit blocking mode on every OS.
                stream.set_nonblocking(false).map_err(|_| "SOCKET_MODE")?;
                Ok(Some(stream))
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err("LISTENER_FAILED"),
        },
        tick,
        handle,
    )
    // listener and accepted streams drop on every exit. No home/key/evidence
    // removal; C's owner drops its Engine to release the existing writer lock.
}

fn drive<S>(
    stop: &AtomicBool,
    limits: Limits,
    mut elapsed: impl FnMut() -> Duration,
    mut sleep: impl FnMut(Duration),
    mut accept: impl FnMut() -> Result<Option<S>, &'static str>,
    mut tick: impl FnMut() -> Result<(), &'static str>,
    mut handle: impl FnMut(S) -> Result<(), &'static str>,
) -> Result<Report, &'static str> {
    limits.validate()?;
    let mut report = Report {
        reason: Stop::Requested,
        handled: 0,
        rejected: 0,
        ticks: 0,
    };
    let mut next_tick = Duration::ZERO;
    loop {
        let now = elapsed();
        if stop.load(Ordering::Acquire) {
            return Ok(report);
        }
        if now >= limits.lifetime {
            report.reason = Stop::Lifetime;
            return Ok(report);
        }
        if report.handled + report.rejected >= limits.requests {
            report.reason = Stop::RequestLimit;
            return Ok(report);
        }
        if now >= next_tick {
            tick()?;
            report.ticks += 1;
            // No catch-up burst after slow IO; scheduling uses monotonic time.
            next_tick = elapsed().saturating_add(TICK);
            // A tick may consume the remaining lifetime or request shutdown.
            continue;
        }
        if let Some(stream) = accept()? {
            // A stop arriving during accept drops the connection unprocessed.
            if stop.load(Ordering::Acquire) {
                return Ok(report);
            }
            if elapsed() >= limits.lifetime {
                report.reason = Stop::Lifetime;
                return Ok(report);
            }
            match handle(stream) {
                Ok(()) => report.handled += 1,
                Err(_) => report.rejected += 1,
            }
        }
        // Throttle even malformed/continuous clients. No unbounded busy-spin.
        sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    fn limits(n: u64) -> Limits {
        Limits {
            lifetime: Duration::from_secs(3),
            requests: n,
        }
    }
    #[test]
    fn disabled_limits_fail_before_any_callback() {
        for l in [
            limits(0),
            limits(100_001),
            Limits {
                lifetime: Duration::ZERO,
                requests: 1,
            },
            Limits {
                lifetime: Duration::from_secs(3601),
                requests: 1,
            },
        ] {
            assert_eq!(
                drive::<()>(
                    &AtomicBool::new(false),
                    l,
                    || panic!(),
                    |_| panic!(),
                    || panic!(),
                    || panic!(),
                    |_| panic!()
                ),
                Err("RESOURCE_LIMIT")
            );
        }
    }
    #[test]
    fn endpoint_is_literal_ipv4_loopback_and_unprivileged() {
        for a in [
            "0.0.0.0:8080",
            "192.168.1.1:8080",
            "[::1]:8080",
            "127.0.0.1:0",
            "127.0.0.1:80",
        ] {
            assert!(endpoint(a.parse().unwrap()).is_err());
        }
        assert!(endpoint("127.0.0.1:65535".parse().unwrap()).is_ok());
    }
    #[test]
    fn early_stop_has_no_effects() {
        let r = drive::<()>(
            &AtomicBool::new(true),
            limits(1),
            || Duration::ZERO,
            |_| panic!(),
            || panic!(),
            || panic!(),
            |_| panic!(),
        )
        .unwrap();
        assert_eq!(
            r,
            Report {
                reason: Stop::Requested,
                handled: 0,
                rejected: 0,
                ticks: 0
            }
        );
    }
    #[test]
    fn worker_failure_never_accepts_or_retries() {
        assert_eq!(
            drive::<()>(
                &AtomicBool::new(false),
                limits(1),
                || Duration::ZERO,
                |_| panic!(),
                || panic!(),
                || Err("RECOVERY_REQUIRED"),
                |_| panic!()
            ),
            Err("RECOVERY_REQUIRED")
        );
    }
    #[test]
    fn tick_before_requests_and_request_budget_counts_rejection() {
        let clock = Cell::new(Duration::ZERO);
        let events = RefCell::new(vec![]);
        let r = drive(
            &AtomicBool::new(false),
            limits(2),
            || clock.get(),
            |d| clock.set(clock.get() + d),
            || {
                events.borrow_mut().push("accept");
                Ok(Some(()))
            },
            || {
                events.borrow_mut().push("tick");
                Ok(())
            },
            |_| {
                events.borrow_mut().push("handle");
                Err("TRANSPORT_CLOSED")
            },
        )
        .unwrap();
        assert_eq!(
            *events.borrow(),
            vec!["tick", "accept", "handle", "accept", "handle"]
        );
        assert_eq!(
            r,
            Report {
                reason: Stop::RequestLimit,
                handled: 0,
                rejected: 2,
                ticks: 1
            }
        );
    }
    #[test]
    fn idle_lifetime_has_bounded_ticks_no_catchup() {
        let clock = Cell::new(Duration::ZERO);
        let r = drive::<()>(
            &AtomicBool::new(false),
            limits(20),
            || clock.get(),
            |d| clock.set(clock.get() + d),
            || Ok(None),
            || {
                clock.set(clock.get() + Duration::from_millis(600));
                Ok(())
            },
            |_| panic!(),
        )
        .unwrap();
        assert_eq!(r.reason, Stop::Lifetime);
        assert_eq!(r.ticks, 2);
    }
    #[test]
    fn listener_failure_is_terminal() {
        assert_eq!(
            drive::<()>(
                &AtomicBool::new(false),
                limits(1),
                || Duration::ZERO,
                |_| panic!(),
                || Err("LISTENER_FAILED"),
                || Ok(()),
                |_| panic!()
            ),
            Err("LISTENER_FAILED")
        );
    }
    #[test]
    fn stop_during_accept_drops_connection_without_dispatch() {
        struct Item<'a>(&'a Cell<bool>);
        impl Drop for Item<'_> {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let stop = AtomicBool::new(false);
        let dropped = Cell::new(false);
        let r = drive(
            &stop,
            limits(1),
            || Duration::ZERO,
            |_| panic!(),
            || {
                stop.store(true, Ordering::Release);
                Ok(Some(Item(&dropped)))
            },
            || Ok(()),
            |_| panic!(),
        )
        .unwrap();
        assert_eq!(r.reason, Stop::Requested);
        assert!(dropped.get());
    }
    #[test]
    fn tick_exhausting_lifetime_never_accepts() {
        let clock = Cell::new(Duration::ZERO);
        let r = drive::<()>(
            &AtomicBool::new(false),
            limits(1),
            || clock.get(),
            |_| panic!(),
            || panic!(),
            || {
                clock.set(Duration::from_secs(3));
                Ok(())
            },
            |_| panic!(),
        )
        .unwrap();
        assert_eq!(r.reason, Stop::Lifetime);
    }
}

//! Parent/child ordering only, NOT an organizational approval or reusable permit.
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub fn await_start(
    mut channel: UnixStream,
    stop: &AtomicBool,
    timeout: Duration,
) -> Result<(), &'static str> {
    if timeout.is_zero() || timeout > Duration::from_secs(5) {
        return Err("START_GATE_LIMIT");
    }
    channel.set_nonblocking(true).map_err(|_| "START_GATE_IO")?;
    let deadline = Instant::now() + timeout;
    let mut written = 0;
    let mut received = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("START_GATE_STOPPED");
        }
        if Instant::now() >= deadline {
            return Err("START_GATE_TIMEOUT");
        }
        if written < 6 {
            match channel.write(&b"READY\n"[written..]) {
                Ok(0) => return Err("START_GATE_IO"),
                Ok(n) => written += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return Err("START_GATE_IO"),
            }
        } else {
            let mut bytes = [0; 7];
            match channel.read(&mut bytes) {
                Ok(0) => {
                    return if received == b"START\n" {
                        Ok(())
                    } else {
                        Err("START_GATE_DENIED")
                    };
                }
                Ok(n) => {
                    received.extend_from_slice(&bytes[..n]);
                    if !b"START\n".starts_with(&received) {
                        return Err("START_GATE_DENIED");
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return Err("START_GATE_IO"),
            }
        }
        std::thread::sleep(
            Duration::from_millis(5).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn exchange(response: &'static [u8], eof: bool) -> Result<(), &'static str> {
        let (child, mut parent) = UnixStream::pair().unwrap();
        let t = std::thread::spawn(move || {
            await_start(child, &AtomicBool::new(false), Duration::from_millis(80))
        });
        let mut ready = [0; 6];
        parent.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"READY\n");
        parent.write_all(response).unwrap();
        if eof {
            parent.shutdown(std::net::Shutdown::Write).unwrap();
        }
        t.join().unwrap()
    }
    #[test]
    fn exact_start_requires_eof() {
        assert_eq!(exchange(b"START\n", true), Ok(()));
        assert_eq!(exchange(b"START\n", false), Err("START_GATE_TIMEOUT"));
    }
    #[test]
    fn deny_truncated_extra_and_closed() {
        for raw in [&b""[..], &b"START"[..], &b"START\nX"[..], &b"GO\n"[..]] {
            assert_eq!(exchange(raw, true), Err("START_GATE_DENIED"));
        }
    }
    #[test]
    fn stop_and_timeout_never_authorize() {
        let (child, _parent) = UnixStream::pair().unwrap();
        assert_eq!(
            await_start(child, &AtomicBool::new(true), Duration::from_secs(1)),
            Err("START_GATE_STOPPED")
        );
        let (child, _parent) = UnixStream::pair().unwrap();
        assert_eq!(
            await_start(child, &AtomicBool::new(false), Duration::from_millis(10)),
            Err("START_GATE_TIMEOUT")
        );
        let (child, _parent) = UnixStream::pair().unwrap();
        assert_eq!(
            await_start(child, &AtomicBool::new(false), Duration::from_secs(6)),
            Err("START_GATE_LIMIT")
        );
    }
}

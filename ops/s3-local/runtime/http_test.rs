#![allow(dead_code)]
#[path = "http.rs"]
mod http;
use http::*;
use std::{
    io::{self, Cursor, Read},
    net::SocketAddr,
    time::{Duration, Instant},
};
#[derive(Default)]
struct Memory {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
    reads: usize,
}
impl Memory {
    fn new(s: impl AsRef<[u8]>) -> Self {
        Self {
            input: Cursor::new(s.as_ref().to_vec()),
            ..Self::default()
        }
    }
}
impl Wire for Memory {
    fn read_until(&mut self, b: &mut [u8], _: Instant) -> io::Result<usize> {
        self.reads += 1;
        let n = b.len().min(3);
        self.input.read(&mut b[..n])
    }
    fn write_until(&mut self, b: &[u8], _: Instant) -> io::Result<usize> {
        let n = b.len().min(7);
        self.output.extend(&b[..n]);
        Ok(n)
    }
}
impl Wire for &mut Memory {
    fn read_until(&mut self, b: &mut [u8], d: Instant) -> io::Result<usize> {
        (**self).read_until(b, d)
    }
    fn write_until(&mut self, b: &[u8], d: Instant) -> io::Result<usize> {
        (**self).write_until(b, d)
    }
}
fn bind() -> SocketAddr {
    "127.0.0.1:8787".parse().unwrap()
}
fn request(extra: &str, body: &str) -> String {
    format!(
        "POST /dev-local/v1/orders HTTP/1.1\r\nHost: 127.0.0.1:8787\r\nContent-Type: application/json\r\n{extra}\r\n{body}"
    )
}
fn parse(s: impl AsRef<[u8]>) -> std::result::Result<Request, Error> {
    decode(
        &mut Memory::new(s),
        bind(),
        bind().ip(),
        Instant::now() + Duration::from_secs(1),
    )
}
#[test]
fn duplicates_are_preserved_for_auth_handler() {
    let r = parse(request(
        "Content-Length: 2\r\nAuthorization: one\r\nauthorization: two\r\n",
        "{}",
    ))
    .unwrap();
    assert_eq!(r.body, b"{}");
    assert_eq!(
        r.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .count(),
        2
    );
}
#[test]
fn ambiguous_lengths_rejected() {
    for h in [
        "Content-Length: 2\r\ncontent-length: 2\r\n",
        "Content-Length: 02\r\n",
        "Content-Length: +2\r\n",
        "Content-Length: 2, 2\r\n",
        "",
    ] {
        assert!(parse(request(h, "{}")).is_err(), "{h}");
    }
}
#[test]
fn all_transfer_coding_rejected() {
    for h in [
        "Transfer-Encoding: chunked",
        "Transfer-Encoding: identity",
        "Expect: 100-continue",
        "Upgrade: websocket",
        "Proxy-Connection: keep-alive",
    ] {
        assert!(parse(request(&format!("Content-Length: 2\r\n{h}\r\n"), "{}")).is_err());
    }
}
#[test]
fn rejects_before_reading_oversize_body() {
    let s = request("Content-Length: 16385\r\n", "SECRET_BODY");
    let mut m = Memory::new(&s);
    assert_eq!(
        decode(
            &mut m,
            bind(),
            bind().ip(),
            Instant::now() + Duration::from_secs(1)
        )
        .unwrap_err(),
        Error::Limit
    );
    assert_eq!(m.input.position() as usize, s.find("SECRET_BODY").unwrap());
}
#[test]
fn exact_body_limit_and_truncation() {
    assert_eq!(
        parse(request("Content-Length: 16384\r\n", &"x".repeat(16384)))
            .unwrap()
            .body
            .len(),
        16384
    );
    assert!(parse(request("Content-Length: 3\r\n", "{}")).is_err());
}
#[test]
fn line_and_header_count_limits() {
    assert!(
        parse(request(
            &format!("Content-Length: 0\r\nX: {}\r\n", "x".repeat(4096)),
            ""
        ))
        .is_err()
    );
    assert!(
        parse(request(
            &format!("Content-Length: 0\r\n{}", "X: y\r\n".repeat(30)),
            ""
        ))
        .is_err()
    );
    assert!(
        parse(request(
            &format!("Content-Length: 0\r\n{}", "X: y\r\n".repeat(29)),
            ""
        ))
        .is_ok()
    );
}
#[test]
fn rejects_obs_fold_controls_and_bad_names() {
    for h in [" X: y", "X : y", "X: a\tb", "X: a\rb", "X: a\0b"] {
        assert!(
            parse(request(&format!("Content-Length: 0\r\n{h}\r\n"), "")).is_err(),
            "{h:?}"
        );
    }
}
#[test]
fn strict_target_and_version() {
    let base = request("Content-Length: 0\r\n", "");
    for p in [
        "http://127.0.0.1/dev-local/v1/orders",
        "/s2/orders",
        "/dev-local/v1/%6frders",
        "/dev-local/v1/orders?x=1",
        "/dev-local/v1/orders#x",
    ] {
        assert!(parse(base.replace("/dev-local/v1/orders", p)).is_err());
    }
    assert!(parse(base.replace("HTTP/1.1", "HTTP/1.0")).is_err());
    assert!(parse(base.replace("\r\n", "\n")).is_err());
}
#[test]
fn binds_host_and_actual_peer() {
    let b = request("Content-Length: 0\r\n", "");
    for host in [
        "evil.example:8787",
        "127.0.0.1:1234",
        "127.0.0.1:8787\r\nHost: 127.0.0.1:8787",
    ] {
        assert_eq!(
            parse(b.replace("127.0.0.1:8787", host)).unwrap_err(),
            Error::Host
        );
    }
    assert_eq!(
        decode(
            &mut Memory::new(b),
            bind(),
            "192.0.2.1".parse().unwrap(),
            Instant::now() + Duration::from_secs(1)
        )
        .unwrap_err(),
        Error::Peer
    );
}
#[test]
fn expired_deadline_reads_nothing() {
    let mut m = Memory::new(request("Content-Length: 0\r\n", ""));
    assert_eq!(
        decode(&mut m, bind(), bind().ip(), Instant::now()).unwrap_err(),
        Error::Deadline
    );
    assert_eq!(m.reads, 0);
}
#[test]
fn pipeline_dispatches_once_and_closes() {
    let b = request(
        "Content-Length: 2\r\nOrigin: http://127.0.0.1:5173\r\n",
        "{}",
    );
    let mut m = Memory::new(format!("{b}{b}"));
    let mut calls = 0;
    serve_one(&mut m, bind(), bind().ip(), |_| {
        calls += 1;
        (200, b"{}".to_vec())
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(m.input.position() as usize, b.len());
    let out = String::from_utf8(m.output).unwrap();
    assert!(out.contains("Connection: close\r\n"));
    assert!(out.contains("Cache-Control: no-store\r\n"));
    assert!(out.contains("Access-Control-Allow-Origin: http://127.0.0.1:5173\r\n"));
}
#[test]
fn cors_does_not_echo_unapproved_or_duplicate_origin() {
    for h in [
        "Origin: https://evil.example\r\n",
        "Origin: http://127.0.0.1:5173\r\nOrigin: http://127.0.0.1:5173\r\n",
    ] {
        let mut m = Memory::new(request(&format!("Content-Length: 0\r\n{h}"), ""));
        serve_one(&mut m, bind(), bind().ip(), |_| (403, b"{}".to_vec())).unwrap();
        assert!(
            !String::from_utf8(m.output)
                .unwrap()
                .contains("Access-Control-Allow-Origin")
        );
    }
    assert!(
        !String::from_utf8(encode(200, b"{}", Some("x\r\nInjected: yes"), false).unwrap())
            .unwrap()
            .contains("Injected")
    );
}
#[test]
fn preflight_never_calls_handler() {
    for (origin, header, status) in [
        (
            "http://localhost:5173",
            "authorization, content-type",
            "204",
        ),
        ("https://evil.example", "authorization", "403"),
        ("http://localhost:5173", "x-owner", "403"),
    ] {
        let mut m = Memory::new(format!(
            "OPTIONS /dev-local/v1/account HTTP/1.1\r\nHost: 127.0.0.1:8787\r\nOrigin: {origin}\r\nAccess-Control-Request-Method: GET\r\nAccess-Control-Request-Headers: {header}\r\n\r\n"
        ));
        serve_one(&mut m, bind(), bind().ip(), |_| {
            panic!("no auth/effect on OPTIONS")
        })
        .unwrap();
        assert!(
            String::from_utf8(m.output)
                .unwrap()
                .starts_with(&format!("HTTP/1.1 {status}"))
        );
    }
}
#[test]
fn framing_rejection_never_dispatches() {
    let mut m = Memory::new(request(
        "Content-Length: 2\r\nTransfer-Encoding: chunked\r\n",
        "{}",
    ));
    serve_one(&mut m, bind(), bind().ip(), |_| {
        panic!("framing rejection must precede handler")
    })
    .unwrap();
    assert!(
        String::from_utf8(m.output)
            .unwrap()
            .starts_with("HTTP/1.1 400")
    );
}

//! Bounded socket adapter for the existing REST and authenticated ChainPort.
//! No listener, auth store, automatic broadcast, or browser-selected anchor.
#[path = "http.rs"] mod http;
#[path = "chain_broadcast.rs"] mod chain_broadcast;
use chain_broadcast::collect;
pub use chain_broadcast::Helper;
use chain_broadcast::chain_http;
use nus_exchange_contract::s3::{dev_local, settlement_local::{Request, Rest}, snapshot::{Observation, Snapshot}};
use serde_json::{Value, json};
use std::net::{IpAddr, SocketAddr, TcpStream};
pub use collect::ChainRead;

fn route<'a>(req: Request<'a>, rest: impl FnOnce(Request<'a>) -> (u16, Value),
    query: impl FnOnce(Request<'a>) -> (u16, Value),
    broadcast: impl FnOnce(Request<'a>) -> (u16, Value)) -> (u16, Value) {
    if req.path == "/dev-local/v1/chain/account" {
        if req.method != "GET" || !req.body.is_empty() {
            return (400, json!({"code":"CHAIN_REQUEST_REJECTED","durable_ack":false}));
        }
        query(req)
    } else if req.path == "/dev-local/v1/chain/result" {
        if req.method != "POST" || req.body.len() > 128 {
            return (400, json!({"code":"CHAIN_REQUEST_REJECTED","durable_ack":false}));
        }
        query(req)
    } else if req.path == "/dev-local/v1/chain/broadcast" {
        if req.method != "POST" {
            return (400, json!({"code":"CHAIN_REQUEST_REJECTED","durable_ack":false}));
        }
        broadcast(req)
    } else if req.path.starts_with("/dev-local/v1/chain/") || req.path == "/dev-local/v1/chain" {
        (404, json!({"code":"CHAIN_ROUTE_UNAVAILABLE","durable_ack":false}))
    } else { rest(req) }
}

fn wire(wire: impl http::Wire, bind: SocketAddr, peer: IpAddr,
    mut clock: impl FnMut() -> dev_local::Result<u64>,
    handle: impl FnOnce(Request<'_>, u64) -> (u16, Value)) -> Result<(), &'static str> {
    http::serve_one(wire, bind, peer, |req| {
        let Ok(now) = clock() else {
            return (503, br#"{"code":"CLOCK_UNAVAILABLE","durable_ack":false}"#.to_vec());
        };
        let headers = req.header_refs();
        let (status, value) = handle(Request { peer, method: &req.method, path: &req.path,
            headers: &headers, body: &req.body }, now);
        match serde_json::to_vec(&value) {
            Ok(body) => (status, body),
            Err(_) => (503, br#"{"code":"RESPONSE_UNAVAILABLE","durable_ack":false}"#.to_vec()),
        }
    }).map_err(|_| "TRANSPORT_CLOSED")
}

/// Consumes one accepted connection. Trusted caller owns the exact validated
/// anchor/REST and the observation's original times. Clock is sampled after read.
pub fn serve(stream: TcpStream, rest: &Rest, observation: &Observation,
    anchor: &Snapshot, chain: &ChainRead, helper: &Helper,
    stop: &std::sync::atomic::AtomicBool,
    mut clock: impl FnMut() -> dev_local::Result<u64>) -> Result<(), &'static str> {
    let bind = stream.local_addr().map_err(|_| "SOCKET_ADDRESS")?;
    let peer = stream.peer_addr().map_err(|_| "SOCKET_ADDRESS")?.ip();
    // Separate borrow for post-read time; the adapter takes subsequent readings
    // during its bounded chain query without replacing observation freshness.
    let clock = std::cell::RefCell::new(&mut clock);
    wire(stream, bind, peer, || (clock.borrow_mut())(), |req, now| {
        route(req, |r| rest.handle(r, observation, now),
            |r| if r.path == "/dev-local/v1/chain/result" {
                chain_broadcast::chain_result::result(rest,r,observation,anchor,chain,stop,|| (clock.borrow_mut())())
            } else {chain_http::account(rest, r, observation, now, anchor, chain,
                || (clock.borrow_mut())())},
            |r| chain_broadcast::broadcast(rest,r,observation,anchor,chain,helper,stop,
                || (clock.borrow_mut())()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{self, Read}, time::Instant};
    struct Memory { input: io::Cursor<Vec<u8>>, output: Vec<u8> }
    impl http::Wire for &mut Memory {
        fn read_until(&mut self, b: &mut [u8], _: Instant) -> io::Result<usize> { self.input.read(b) }
        fn write_until(&mut self, b: &[u8], _: Instant) -> io::Result<usize> {
            self.output.extend_from_slice(b); Ok(b.len())
        }
    }
    fn input(method: &str, path: &str, body: &str) -> Memory {
        Memory { input: io::Cursor::new(format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:8787\r\nOrigin: http://localhost:5173\r\nAuthorization: Bearer first\r\nAuthorization: Bearer second\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len()).into_bytes()), output: vec![] }
    }
    fn run(m: &mut Memory, handle: impl FnOnce(Request<'_>,u64)->(u16,Value)) {
        wire(m,"127.0.0.1:8787".parse().unwrap(),"127.0.0.1".parse().unwrap(),||Ok(42),handle).unwrap();
    }
    #[test]
    fn socket_request_preserves_peer_headers_and_account_route() {
        let mut m=input("GET","/dev-local/v1/chain/account","");
        run(&mut m,|r,now|route(r,|_|panic!("rest fallback"),|r| {
            assert_eq!(now,42); assert!(r.peer.is_loopback());
            assert_eq!(r.headers.iter().filter(|(k,_)|k.eq_ignore_ascii_case("authorization")).count(),2);
            (401,json!({"code":"AUTH_REJECTED","durable_ack":false}))
        }, |_|panic!("broadcast")));
        let out=String::from_utf8(m.output).unwrap();
        assert!(out.starts_with("HTTP/1.1 401")); assert!(!out.contains("Bearer"));
        assert!(out.contains("http://localhost:5173"));
    }
    #[test]
    fn broadcast_preserves_exact_wire_and_never_uses_rest_fallback() {
        let mut m=input("POST","/dev-local/v1/chain/broadcast",r#"{"tx_bytes":"YWJj"}"#);
        let mut calls=0;
        run(&mut m,|r,_|route(r,|_|panic!("rest"),|_|panic!("account"),|r| {
            calls+=1;
            assert_eq!(r.body,br#"{"tx_bytes":"YWJj"}"#);
            assert_eq!(r.headers.iter().filter(|(k,_)|k.eq_ignore_ascii_case("authorization")).count(),2);
            assert!(r.peer.is_loopback());
            (409,json!({"code":"CHAIN_BROADCAST_REJECTED","durable_ack":false}))
        }));
        assert_eq!(calls,1);
        let out=String::from_utf8(m.output).unwrap();
        assert!(out.starts_with("HTTP/1.1 409"));
        assert!(!out.contains("Bearer"));
        assert!(!out.contains("COMMITTED"));
    }
    #[test]
    fn result_dispatch_preserves_exact_body_and_auth_headers() {
        let body=r#"{"tx_hash":"abc"}"#;
        let mut m=input("POST","/dev-local/v1/chain/result",body);
        run(&mut m,|r,_|route(r,|_|panic!("rest"),|r| {
            assert_eq!(r.path,"/dev-local/v1/chain/result");
            assert_eq!(r.body,body.as_bytes());
            assert_eq!(r.headers.iter().filter(|(k,_)|k.eq_ignore_ascii_case("authorization")).count(),2);
            (409,json!({"code":"CHAIN_RESULT_UNAVAILABLE","durable_ack":false}))
        },|_|panic!("broadcast")));
        assert!(String::from_utf8(m.output).unwrap().starts_with("HTTP/1.1 409"));
    }
    #[test]
    fn broadcast_transport_rejection_never_dispatches() {
        let mut m=input("POST","/dev-local/v1/chain/broadcast",r#"{"tx_bytes":"YWJj"}"#);
        wire(&mut m,"127.0.0.1:8787".parse().unwrap(),"192.0.2.1".parse().unwrap(),
            ||panic!("clock"),|_,_|panic!("dispatch")).unwrap();
        assert!(String::from_utf8(m.output).unwrap().starts_with("HTTP/1.1 400"));
    }
    #[test]
    fn malformed_chain_routes_never_reach_handlers() {
        for (method,path,body,status) in [
            ("GET","/dev-local/v1/chain/result","",400),
            ("POST","/dev-local/v1/chain/result/","{}",404),
            ("POST","/dev-local/v1/chain/account","",400),
            ("GET","/dev-local/v1/chain/account","{}",400),
            ("GET","/dev-local/v1/chain/account/","",404),
            ("GET","/dev-local/v1/chain/broadcast","{}",400),
            ("POST","/dev-local/v1/chain/broadcast/","{}",404)] {
            let mut m=input(method,path,body);
            run(&mut m,|r,_|route(r,|_|panic!("rest"),|_|panic!("query"),|_|panic!("broadcast")));
            assert!(String::from_utf8(m.output).unwrap().starts_with(&format!("HTTP/1.1 {status}")));
        }
    }
    #[test]
    fn existing_rest_routes_and_body_unchanged() {
        let mut m=input("POST","/dev-local/v1/login",r#"{"proof":"test"}"#);
        run(&mut m,|r,_|route(r,|r| {
            assert_eq!(r.path,"/dev-local/v1/login"); assert_eq!(r.body,br#"{"proof":"test"}"#);
            (403,json!({"code":"REJECTED"}))
        },|_|panic!("chain"),|_|panic!("broadcast")));
        assert!(String::from_utf8(m.output).unwrap().starts_with("HTTP/1.1 403"));
    }
    #[test]
    fn clock_and_transport_failures_do_not_dispatch() {
        let mut m=input("GET","/dev-local/v1/chain/account","");
        wire(&mut m,"127.0.0.1:8787".parse().unwrap(),"127.0.0.1".parse().unwrap(),
            ||Err("CLOCK".into()),|_,_|panic!("dispatch")).unwrap();
        assert!(String::from_utf8(m.output).unwrap().starts_with("HTTP/1.1 503"));
        let mut m=input("GET","/dev-local/v1/chain/account","");
        wire(&mut m,"127.0.0.1:8787".parse().unwrap(),"192.0.2.1".parse().unwrap(),
            ||panic!("clock"),|_,_|panic!("dispatch")).unwrap();
        assert!(String::from_utf8(m.output).unwrap().starts_with("HTTP/1.1 400"));
    }
}

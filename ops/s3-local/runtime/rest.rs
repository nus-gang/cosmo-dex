//! Thin SRE adapter. No owner/peer/observation obtained from browser fields.
//! Runtime construction must first validate the exact approved input set/home.
#[path = "http.rs"]
mod http;
use nus_exchange_contract::s3::{
    settlement_local::{Request, Rest},
    snapshot::Observation,
};
use std::{
    net::TcpStream,
    time::{SystemTime, UNIX_EPOCH},
};

/// A single accepted socket is consumed and closed on every return. There is no
/// listener/startup here. Keep the original trusted observation timestamps;
/// obtain wall time after the bounded request read, never when it was accepted.
/// RPC failure must leave observation absent or retain its original old times.
pub fn serve_rest(
    stream: TcpStream,
    rest: &Rest,
    observation: &Observation,
) -> Result<(), &'static str> {
    let bind = stream.local_addr().map_err(|_| "SOCKET_ADDRESS")?;
    let peer = stream.peer_addr().map_err(|_| "SOCKET_ADDRESS")?.ip();
    http::serve_one(stream, bind, peer, |req| {
        let now = match SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_millis()).ok())
        {
            Some(now) => now,
            None => {
                return (
                    503,
                    br#"{"code":"CLOCK_UNAVAILABLE","durable_ack":false}"#.to_vec(),
                );
            }
        };
        let headers = req.header_refs();
        let (status, value) = rest.handle(
            Request {
                peer,
                method: &req.method,
                path: &req.path,
                headers: &headers,
                body: &req.body,
            },
            observation,
            now,
        );
        match serde_json::to_vec(&value) {
            Ok(body) => (status, body),
            Err(_) => (
                503,
                br#"{"code":"RESPONSE_UNAVAILABLE","durable_ack":false}"#.to_vec(),
            ),
        }
    })
    .map_err(|_| "TRANSPORT_CLOSED")
}

#[path = "lifecycle.rs"]
pub mod lifecycle;

/// Connect the bounded accept loop to the reviewed REST component. The trusted
/// callback must perform reconciliation before returning its original observed
/// timestamps. Errors end this loop without refreshing or exposing stale state.
/// The callback must not swallow RECOVERY_REQUIRED as a successful observation.
/// Startup, signal registration and independent approval are caller obligations.
pub fn serve_service(
    listener: std::net::TcpListener,
    expected: std::net::SocketAddr,
    stop: &std::sync::atomic::AtomicBool,
    limits: lifecycle::Limits,
    rest: &Rest,
    mut trusted_tick: impl FnMut() -> Result<Observation, &'static str>,
) -> Result<lifecycle::Report, &'static str> {
    let observation = std::cell::RefCell::new(None);
    lifecycle::serve(
        listener,
        expected,
        stop,
        limits,
        || {
            let next = trusted_tick()?;
            *observation.borrow_mut() = Some(next);
            Ok(())
        },
        |stream| {
            let borrowed = observation.borrow();
            let observed = borrowed.as_ref().ok_or("OBSERVATION_UNAVAILABLE")?;
            serve_rest(stream, rest, observed)
        },
    )
}

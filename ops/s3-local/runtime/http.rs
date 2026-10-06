//! L-R transport only: one bounded HTTP/1.1 request per connection. No listener.
//! The runtime caller supplies the actual peer and a trusted chain observation.
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub const BODY_LIMIT: usize = 16 * 1024;
const LINE_LIMIT: usize = 4096;
const HEADER_LIMIT: usize = 32;
const DEADLINE: Duration = Duration::from_secs(2);
const ORIGINS: [&str; 2] = ["http://127.0.0.1:5173", "http://localhost:5173"];
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Io,
    Deadline,
    Framing,
    Limit,
    Peer,
    Host,
}
type Result<T> = std::result::Result<T, Error>;

/// Implementations must bound each IO operation by the remaining TOTAL deadline.
pub trait Wire {
    fn read_until(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<usize>;
    fn write_until(&mut self, buf: &[u8], deadline: Instant) -> io::Result<usize>;
}
impl Wire for TcpStream {
    fn read_until(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<usize> {
        self.set_read_timeout(Some(remaining(deadline)?))?;
        self.read(buf)
    }
    fn write_until(&mut self, buf: &[u8], deadline: Instant) -> io::Result<usize> {
        self.set_write_timeout(Some(remaining(deadline)?))?;
        self.write(buf)
    }
}
fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "deadline"))
}
fn read(wire: &mut impl Wire, buf: &mut [u8], deadline: Instant) -> Result<usize> {
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    wire.read_until(buf, deadline).map_err(|_| Error::Io)
}
fn line(wire: &mut impl Wire, deadline: Instant) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        // Includes CRLF; no unbounded read_until allocation or prefetch.
        if out.len() == LINE_LIMIT {
            return Err(Error::Limit);
        }
        let mut b = [0];
        if read(wire, &mut b, deadline)? != 1 {
            return Err(Error::Framing);
        }
        out.push(b[0]);
        if b[0] == b'\n' {
            if !out.ends_with(b"\r\n") {
                return Err(Error::Framing);
            }
            out.truncate(out.len() - 2);
            return Ok(out);
        }
    }
}
fn token(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}
#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
impl Request {
    pub fn header_refs(&self) -> Vec<(&str, &str)> {
        self.headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
    pub fn approved_origin(&self) -> Option<&str> {
        let mut it = self
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("origin"));
        let (_, v) = it.next()?;
        if it.next().is_some() || !ORIGINS.contains(&v.as_str()) {
            return None;
        }
        Some(v)
    }
}
/// Caller binds an explicitly selected literal loopback address. Require Host to
/// match that socket (localhost alias allowed); no forwarded-host trust or DNS.
pub fn decode(
    wire: &mut impl Wire,
    bind: SocketAddr,
    peer: IpAddr,
    deadline: Instant,
) -> Result<Request> {
    if !peer.is_loopback() || !bind.ip().is_loopback() || bind.port() == 0 {
        return Err(Error::Peer);
    }
    let first = line(wire, deadline)?;
    let first = std::str::from_utf8(&first).map_err(|_| Error::Framing)?;
    let parts: Vec<_> = first.split(' ').collect();
    if parts.len() != 3
        || parts[2] != "HTTP/1.1"
        || !["GET", "POST", "OPTIONS"].contains(&parts[0])
        || !parts[1].starts_with("/dev-local/v1/")
        || parts[1]
            .bytes()
            .any(|b| b <= 32 || b >= 127 || b"%?#\\".contains(&b))
    {
        return Err(Error::Framing);
    }
    let mut headers = Vec::new();
    loop {
        let raw = line(wire, deadline)?;
        if raw.is_empty() {
            break;
        }
        if headers.len() == HEADER_LIMIT {
            return Err(Error::Limit);
        }
        let s = std::str::from_utf8(&raw).map_err(|_| Error::Framing)?;
        let (k, v) = s.split_once(':').ok_or(Error::Framing)?;
        if k.is_empty() || !k.bytes().all(token) || v.bytes().any(|b| b < 32 || b == 127) {
            return Err(Error::Framing);
        }
        headers.push((k.to_owned(), v.trim_matches(' ').to_owned()));
    }
    let values = |name: &str| -> Vec<&str> {
        headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    };
    let hosts = values("host");
    if hosts.len() != 1
        || !(hosts[0] == bind.to_string() || hosts[0] == format!("localhost:{}", bind.port()))
    {
        return Err(Error::Host);
    }
    // Never support transfer coding, upgrades, Expect/100-continue, proxy framing.
    if ["transfer-encoding", "expect", "upgrade", "proxy-connection"]
        .iter()
        .any(|k| !values(k).is_empty())
    {
        return Err(Error::Framing);
    }
    let lengths = values("content-length");
    if lengths.len() > 1 || (parts[0] == "POST" && lengths.len() != 1) {
        return Err(Error::Framing);
    }
    let n = if let Some(s) = lengths.first() {
        if s.is_empty()
            || !s.bytes().all(|b| b.is_ascii_digit())
            || (s.len() > 1 && s.starts_with('0'))
        {
            return Err(Error::Framing);
        }
        s.parse::<usize>().map_err(|_| Error::Limit)?
    } else {
        0
    };
    if n > BODY_LIMIT {
        return Err(Error::Limit);
    }
    if parts[0] != "POST" && n != 0 {
        return Err(Error::Framing);
    }
    if parts[0] == "POST" {
        let types = values("content-type");
        if types.len() != 1 || types[0] != "application/json" {
            return Err(Error::Framing);
        }
    }
    let mut body = vec![0; n];
    let mut at = 0;
    while at < n {
        let n = read(wire, &mut body[at..], deadline)?;
        if n == 0 {
            return Err(Error::Framing);
        }
        at += n;
    }
    Ok(Request {
        method: parts[0].into(),
        path: parts[1].into(),
        headers,
        body,
    })
}
/// No untrusted string may become a response header. Origin is exact allowlist.
pub fn encode(status: u16, body: &[u8], origin: Option<&str>, preflight: bool) -> Result<Vec<u8>> {
    if body.len() > 2 * 1024 * 1024 {
        return Err(Error::Limit);
    }
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        503 => "Service Unavailable",
        _ => return Err(Error::Framing),
    };
    let mut s = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\nContent-Length: {}\r\nVary: Origin\r\n",
        body.len()
    );
    if let Some(origin) = origin.filter(|o| ORIGINS.contains(o)) {
        s.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
        if preflight {
            s.push_str("Access-Control-Allow-Methods: GET, POST\r\nAccess-Control-Allow-Headers: authorization, content-type\r\n");
        }
    }
    s.push_str("\r\n");
    Ok([s.as_bytes(), body].concat())
}
/// Own and close the stream at return; no keepalive, pipelined second dispatch,
/// listener, thread creation or trusted observation construction here.
pub fn serve_one(
    mut wire: impl Wire,
    bind: SocketAddr,
    peer: IpAddr,
    handle: impl FnOnce(&Request) -> (u16, Vec<u8>),
) -> Result<()> {
    let deadline = Instant::now() + DEADLINE;
    let response = match decode(&mut wire, bind, peer, deadline) {
        Ok(req) => {
            let origin = req.approved_origin();
            if req.method == "OPTIONS" {
                let hs = req.header_refs();
                let methods: Vec<_> = hs
                    .iter()
                    .filter(|(k, _)| k.eq_ignore_ascii_case("access-control-request-method"))
                    .collect();
                let requested: Vec<_> = hs
                    .iter()
                    .filter(|(k, _)| k.eq_ignore_ascii_case("access-control-request-headers"))
                    .collect();
                let valid = origin.is_some()
                    && methods.len() == 1
                    && ["GET", "POST"].contains(&methods[0].1)
                    && requested.len() <= 1
                    && requested.iter().all(|(_, v)| {
                        v.split(',').all(|h| {
                            ["authorization", "content-type"]
                                .contains(&h.trim().to_ascii_lowercase().as_str())
                        })
                    });
                encode(
                    if valid { 204 } else { 403 },
                    b"",
                    if valid { origin } else { None },
                    valid,
                )?
            } else {
                let (status, body) = handle(&req);
                encode(status, &body, origin, false)?
            }
        }
        Err(e) => encode(
            if e == Error::Limit { 413 } else { 400 },
            b"{\"code\":\"HTTP_REJECTED\",\"durable_ack\":false}",
            None,
            false,
        )?,
    };
    let mut at = 0;
    while at < response.len() {
        if Instant::now() >= deadline {
            return Err(Error::Deadline);
        }
        let n = wire
            .write_until(&response[at..], deadline)
            .map_err(|_| Error::Io)?;
        if n == 0 {
            return Err(Error::Io);
        }
        at += n;
    }
    Ok(())
}

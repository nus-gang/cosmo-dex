//! SRE read-only Comet transport. Raw JSON remains evidence for C's validators.
//! No broadcast/signing endpoint, engine mutation, retry, or finality inference.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
const BODY_LIMIT: usize = 16_777_216; // inherited evidence::RPC limit
const HEADER_LIMIT: usize = 16_384;
const WIRE_LIMIT: usize = BODY_LIMIT + 262_144;
type Result<T> = std::result::Result<T, &'static str>;

pub enum Query<'a> {
    Abci {
        path: &'a str,
        data_hex: &'a str,
        height: u64,
    },
    Block(u64),
    BlockResults(u64),
    Tx(&'a str),
}
fn hex(s: &str) -> bool {
    !s.is_empty() && s.len() % 2 == 0 && s.bytes().all(|c| c.is_ascii_hexdigit())
}
impl Query<'_> {
    fn body(&self) -> Result<Vec<u8>> {
        let (method, params): (&str, Value) = match self {
            Self::Abci {
                path,
                data_hex,
                height,
            } => {
                if ![
                    "/nus.exchange.s3.v1.Query/Snapshot",
                    "/nus.exchange.s3.v1.Query/Batch",
                    "/nus.exchange.s3.v1.Query/Order",
                    "/cosmos.auth.v1beta1.Query/Account",
                    "/cosmos.bank.v1beta1.Query/Balance",
                    "/nus.exchange.v1.Query/Receipt",
                ]
                .contains(path)
                    || data_hex.len() > 32768
                    || !hex(data_hex)
                {
                    return Err("QUERY_POLICY");
                }
                (
                    "abci_query",
                    json!({"path":path,"data":data_hex,"height":height.to_string(),"prove":false}),
                )
            }
            Self::Block(h) | Self::BlockResults(h) => {
                if *h == 0 {
                    return Err("EXACT_HEIGHT_REQUIRED");
                }
                (
                    if matches!(self, Self::Block(_)) {
                        "block"
                    } else {
                        "block_results"
                    },
                    json!({"height":h.to_string()}),
                )
            }
            Self::Tx(hash) => {
                if hash.len() != 64 || !hex(hash) {
                    return Err("TX_HASH");
                }
                // Comet JSON-RPC bytes use base64; encode exact decoded SHA256.
                let bytes: Vec<u8> = (0..64)
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hash[i..i + 2], 16).unwrap())
                    .collect();
                use base64::Engine as _;
                (
                    "tx",
                    json!({"hash":base64::engine::general_purpose::STANDARD.encode(bytes),"prove":false}),
                )
            }
        };
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .map_err(|_| "QUERY_ENCODING")
    }
}

pub struct QueryRpc {
    addr: SocketAddr,
}
impl QueryRpc {
    pub fn new(addr: SocketAddr) -> Result<Self> {
        if !addr.ip().is_loopback() || addr.port() < 1024 {
            return Err("QUERY_POLICY");
        }
        Ok(Self { addr })
    }
    /// Two seconds over connect + write + read. No DNS/proxy/redirect; returns
    /// unchanged JSON entity bytes, not canonicalized or reserialized evidence.
    pub fn fetch(&self, query: Query<'_>) -> Result<Vec<u8>> {
        let body = query.body()?;
        let end = Instant::now() + Duration::from_secs(2);
        let mut socket =
            TcpStream::connect_timeout(&self.addr, remaining(end)?).map_err(|_| "QUERY_CONNECT")?;
        let mut request = format!("POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", self.addr, body.len()).into_bytes();
        request.extend(body);
        let mut sent = 0;
        while sent < request.len() {
            socket
                .set_write_timeout(Some(remaining(end)?))
                .map_err(|_| "QUERY_IO")?;
            let n = socket.write(&request[sent..]).map_err(|_| "QUERY_IO")?;
            if n == 0 {
                return Err("QUERY_EOF");
            }
            sent += n;
        }
        let mut wire = Vec::new();
        let mut header_complete = false;
        let mut buf = [0u8; 8192];
        loop {
            socket
                .set_read_timeout(Some(remaining(end)?))
                .map_err(|_| "QUERY_IO")?;
            let n = socket.read(&mut buf).map_err(|_| "QUERY_IO")?;
            if n == 0 {
                break;
            }
            if wire.len() + n > WIRE_LIMIT {
                return Err("QUERY_SIZE");
            }
            wire.extend_from_slice(&buf[..n]);
            if !header_complete {
                header_complete = wire.windows(4).any(|w| w == b"\r\n\r\n");
            }
            if !header_complete && wire.len() > HEADER_LIMIT {
                return Err("QUERY_HEADER");
            }
        }
        remaining(end)?;
        decode_http(&wire)
    }
}
fn remaining(end: Instant) -> Result<Duration> {
    end.checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or("QUERY_TIMEOUT")
}
fn decimal(s: &str) -> Result<usize> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("QUERY_LENGTH");
    }
    s.parse().map_err(|_| "QUERY_LENGTH")
}
fn decode_http(wire: &[u8]) -> Result<Vec<u8>> {
    if wire.len() > WIRE_LIMIT {
        return Err("QUERY_SIZE");
    }
    let end = wire
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("QUERY_HEADER")?;
    if end + 4 > HEADER_LIMIT {
        return Err("QUERY_HEADER");
    }
    let header = std::str::from_utf8(&wire[..end]).map_err(|_| "QUERY_HEADER")?;
    let mut lines = header.split("\r\n");
    if lines.next() != Some("HTTP/1.1 200 OK") {
        return Err("QUERY_STATUS");
    }
    let mut length = None;
    let mut chunked = false;
    let mut content_type = false;
    let mut count = 0;
    for line in lines {
        count += 1;
        if count > 64 || line.len() > 4096 || line.bytes().any(|b| b < 32 || b == 127) {
            return Err("QUERY_HEADER");
        }
        let (name, value) = line.split_once(':').ok_or("QUERY_HEADER")?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("QUERY_HEADER");
        }
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => {
                if length.is_some() {
                    return Err("QUERY_FRAMING");
                }
                length = Some(decimal(value)?);
            }
            "transfer-encoding" => {
                if chunked || !value.eq_ignore_ascii_case("chunked") {
                    return Err("QUERY_FRAMING");
                }
                chunked = true;
            }
            "content-type" => {
                if content_type
                    || !value
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .eq_ignore_ascii_case("application/json")
                {
                    return Err("QUERY_CONTENT_TYPE");
                }
                content_type = true;
            }
            "content-encoding" | "trailer" | "upgrade" => return Err("QUERY_ENCODING"),
            _ => {}
        }
    }
    if !content_type || (length.is_some() && chunked) {
        return Err("QUERY_FRAMING");
    }
    let body = &wire[end + 4..];
    let raw = if chunked {
        let mut rest = body;
        let mut out = Vec::new();
        let mut chunks = 0;
        loop {
            chunks += 1;
            if chunks > 32768 {
                return Err("QUERY_CHUNKS");
            }
            let pos = rest
                .windows(2)
                .position(|w| w == b"\r\n")
                .ok_or("QUERY_CHUNK")?;
            if pos == 0 || pos > 8 || !rest[..pos].iter().all(|b| b.is_ascii_hexdigit()) {
                return Err("QUERY_CHUNK");
            }
            let n = usize::from_str_radix(
                std::str::from_utf8(&rest[..pos]).map_err(|_| "QUERY_CHUNK")?,
                16,
            )
            .map_err(|_| "QUERY_CHUNK")?;
            rest = &rest[pos + 2..];
            if n == 0 {
                if rest != b"\r\n" {
                    return Err("QUERY_TRAILING");
                }
                break;
            }
            if n > BODY_LIMIT - out.len() {
                return Err("QUERY_SIZE");
            }
            if rest.len() < n + 2 || &rest[n..n + 2] != b"\r\n" {
                return Err("QUERY_CHUNK");
            }
            out.extend_from_slice(&rest[..n]);
            rest = &rest[n + 2..];
        }
        out
    } else {
        let n = length.ok_or("QUERY_FRAMING")?;
        if n > BODY_LIMIT {
            return Err("QUERY_SIZE");
        }
        if n != body.len() {
            return Err("QUERY_LENGTH");
        }
        body.to_vec()
    };
    if raw.is_empty() {
        return Err("QUERY_EMPTY");
    }
    // Duplicate-key and typed proof checks belong to the existing C codec.
    // A JSON-RPC error (including NOT_FOUND) stays raw, never becomes a receipt.
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixed(body: &[u8]) -> Vec<u8> {
        [
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .into_bytes(),
            body.to_vec(),
        ]
        .concat()
    }
    #[test]
    fn exact_entity_bytes() {
        let b = b"{ \"result\": null }\n";
        assert_eq!(decode_http(&fixed(b)).unwrap(), b);
    }
    #[test]
    fn rpc_error_stays_error_bytes() {
        let b = br#"{"error":{"code":-32603}}"#;
        assert_eq!(decode_http(&fixed(b)).unwrap(), b);
    }
    #[test]
    fn chunked_entity_bytes() {
        assert_eq!(decode_http(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n{\r\n1\r\n}\r\n0\r\n\r\n").unwrap(), b"{}");
    }
    #[test]
    fn truncated_and_extra_rejected() {
        let mut w = fixed(b"{}");
        w.pop();
        assert!(decode_http(&w).is_err());
        w.extend(b"}extra");
        assert!(decode_http(&w).is_err());
    }
    #[test]
    fn ambiguous_framing_rejected() {
        for h in [
            "Content-Length: 2\r\nContent-Length: 2",
            "Content-Length: 2\r\nTransfer-Encoding: chunked",
            "Transfer-Encoding: gzip",
            "Content-Length: +2",
            "Content-Length: 16777217",
        ] {
            assert!(
                decode_http(
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{h}\r\n\r\n{{}}")
                        .as_bytes()
                )
                .is_err()
            );
        }
    }
    #[test]
    fn chunk_abuse_rejected() {
        for b in [
            "1;x=y\r\n{\r\n0\r\n\r\n",
            "2\r\n{}\r\n0\r\nX: x\r\n\r\n",
            "1000001\r\n",
            "2\r\n{",
            "0\r\n\r\nextra",
        ] {
            assert!(decode_http(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{b}").as_bytes()).is_err());
        }
    }
    #[test]
    fn status_encoding_and_header_rejected() {
        for h in [
            "HTTP/1.1 302 Found\r\nContent-Type: application/json",
            "HTTP/1.1 200 OK\r\nContent-Type: text/html",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip",
            "HTTP/1.1 200 OK\r\n Content-Type: application/json",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nBad Header: value",
        ] {
            assert!(
                decode_http(format!("{h}\r\nContent-Length: 2\r\n\r\n{{}}").as_bytes()).is_err()
            );
        }
    }
    #[test]
    fn endpoint_and_deadline() {
        for a in ["0.0.0.0:1234", "192.0.2.1:1234", "127.0.0.1:80"] {
            assert!(QueryRpc::new(a.parse().unwrap()).is_err());
        }
        assert!(QueryRpc::new("[::1]:26657".parse().unwrap()).is_ok());
        assert!(remaining(Instant::now() - Duration::from_secs(1)).is_err());
    }
    #[test]
    fn fixed_height_and_query_allowlist() {
        assert!(Query::Block(0).body().is_err());
        assert!(Query::BlockResults(0).body().is_err());
        assert!(
            Query::Abci {
                path: "broadcast_tx_sync",
                data_hex: "00",
                height: 1
            }
            .body()
            .is_err()
        );
        assert!(
            Query::Abci {
                path: "/nus.exchange.s3.v1.Query/Snapshot",
                data_hex: "0",
                height: 1
            }
            .body()
            .is_err()
        );
        let v: Value = serde_json::from_slice(&Query::Block(2).body().unwrap()).unwrap();
        assert_eq!(v["params"]["height"], "2");
    }
    #[test]
    fn tx_hash_encoding() {
        assert!(Query::Tx("aa").body().is_err());
        let v: Value =
            serde_json::from_slice(&Query::Tx(&"00".repeat(32)).body().unwrap()).unwrap();
        assert_eq!(
            v["params"]["hash"],
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );
    }
}

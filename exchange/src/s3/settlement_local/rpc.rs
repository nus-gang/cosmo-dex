//! Finite, no-proxy/no-DNS/no-redirect Comet transport. No Engine access.
use crate::s3::dev_local::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
const LIMIT: usize = 65536;
pub struct LoopbackRpc {
    addr: SocketAddr,
    timeout: Duration,
}
impl LoopbackRpc {
    pub fn new(addr: SocketAddr, timeout: Duration) -> Result<Self> {
        if !addr.ip().is_loopback()
            || addr.port() == 0
            || timeout.is_zero()
            || timeout > Duration::from_secs(2)
        {
            return Err(Error::Invalid("RPC_POLICY"));
        }
        Ok(Self { addr, timeout })
    }
    pub(super) fn broadcast(&self, raw: &[u8]) -> Result<()> {
        if raw.len() > 139264 {
            return Err(Error::Invalid("TX_LIMIT"));
        }
        let end = Instant::now() + self.timeout;
        let mut socket = TcpStream::connect_timeout(&self.addr, self.timeout)?;
        let body = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"broadcast_tx_sync","params":{"tx":STANDARD.encode(raw)}})).map_err(|_| Error::Invalid("RPC_ENCODING"))?;
        let mut bytes = format!("POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",self.addr,body.len()).into_bytes();
        bytes.extend(body);
        let remaining = || {
            end.checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or(Error::Invalid("RPC_TIMEOUT"))
        };
        let mut offset = 0;
        while offset < bytes.len() {
            socket.set_write_timeout(Some(remaining()?))?;
            let n = socket.write(&bytes[offset..])?;
            if n == 0 {
                return Err(Error::Invalid("RPC_EOF"));
            }
            offset += n;
        }
        let mut total = 0;
        let mut buffer = [0u8; 4096];
        loop {
            socket.set_read_timeout(Some(remaining()?))?;
            let n = socket.read(&mut buffer)?;
            if n == 0 {
                return Ok(());
            }
            total += n;
            if total > LIMIT {
                return Err(Error::Invalid("RPC_RESPONSE_LIMIT"));
            }
        }
    }
}

//! Single-threaded process boundary for the Settlement HTTP/RPC adapter.
//! Only the trusted parent owns this pipe. Browser input enters `Request`;
//! it must never be forwarded as an arbitrary control message.
use super::{
    auth::Auth,
    journal::{self, Error},
    recovery::SignedRecovery,
    request::{self, SignedCommand, SignedKind},
    service::Service,
    snapshot::{Binding, Observation},
};
use crate::codec;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::Path,
};

pub const MAX_CONTROL_BYTES: usize = 100_000;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub context: Value,
    pub market: Value,
    pub owners: [String; 2],
    pub supplies: [String; 2],
    pub bootstrap_snapshot_id: String,
}
impl Manifest {
    pub fn decode(raw: &[u8]) -> crate::Result<Self> {
        if raw.len() > 65_536 {
            return Err("RESOURCE_LIMIT");
        }
        let value = codec::unique_json(raw)?;
        let m: Self = serde_json::from_value(value).map_err(|_| "MANIFEST_SCHEMA")?;
        let pinned: Value =
            serde_json::from_str(include_str!("../../../protocol/s2/manifest.json"))
                .map_err(|_| "MANIFEST_SCHEMA")?;
        if m.context["contract_hash"] != pinned["contract_sha256"]
            || m.context["config_hash"] != pinned["config_sha256"]
            || m.context["chain_id"] != "nus-s2-dev-1"
            || !is_hash(&m.bootstrap_snapshot_id)
        {
            return Err("CONTEXT_MISMATCH");
        }
        m.binding()?;
        Ok(m)
    }
    pub fn binding(&self) -> crate::Result<Binding> {
        let owners = [
            request::bytes(&json!(self.owners[0]))?,
            request::bytes(&json!(self.owners[1]))?,
        ];
        let supplies = [
            codec::integer(&json!(self.supplies[0]), 128)?,
            codec::integer(&json!(self.supplies[1]), 128)?,
        ];
        Binding::new(self.context.clone(), self.market.clone(), owners, supplies)
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Request {
        method: String,
        path: String,
        origin: Option<String>,
        authorization: Option<String>,
        body_base64: String,
    },
    Observe {
        snapshot: Value,
        cursor_height: String,
        received_at_unix_ms: String,
        query_latency_ms: String,
        catching_up: bool,
    },
    RpcFailed {},
}
pub struct Runtime {
    service: Service,
    auth: Auth,
    binding: Binding,
}
fn invalid(code: &'static str) -> Error {
    Error::InvalidRecord(code)
}
fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Runtime {
    pub fn start(
        manifest: &Manifest,
        dir: &Path,
        bootstrap: Option<&[u8]>,
    ) -> journal::Result<Self> {
        let binding = manifest.binding().map_err(invalid)?;
        let engine = if let Some(raw) = bootstrap {
            let snapshot = binding.decode(raw).map_err(invalid)?;
            if snapshot.id() != manifest.bootstrap_snapshot_id {
                return Err(invalid("BOOTSTRAP_MISMATCH"));
            }
            SignedRecovery::create(
                dir,
                super::sequencer::Candidate::new(snapshot).map_err(invalid)?,
                "OPEN",
            )?
        } else {
            SignedRecovery::open_persisted(dir, &binding, &manifest.bootstrap_snapshot_id, "OPEN")?
        };
        let auth = Auth::new(engine.state().snapshot());
        let mut service = Service::new(engine);
        // Validate/reserve revisions at startup; never admit commands with a
        // missing or uncertain counter, even before the first status GET.
        service.status(0)?;
        Ok(Self {
            service,
            auth,
            binding,
        })
    }
    pub fn service(&self) -> &Service {
        &self.service
    }
    pub fn handle(&mut self, raw: &[u8], now: u64) -> Value {
        let result = self.message(raw, now);
        match result {
            Ok((status, body)) => json!({"http_status":status.to_string(), "body":body}),
            Err(error) => self.error(error),
        }
    }
    fn message(&mut self, raw: &[u8], now: u64) -> journal::Result<(u16, Value)> {
        if raw.len() > MAX_CONTROL_BYTES {
            return Err(Error::ResourceLimit);
        }
        let message: Message = serde_json::from_value(
            codec::unique_json(raw).map_err(|_| invalid("NON_CANONICAL_WIRE"))?,
        )
        .map_err(|_| invalid("NON_CANONICAL_WIRE"))?;
        match message {
            Message::RpcFailed {} => {
                self.service.rpc_failed();
                Ok((200, json!({"observed":false})))
            }
            Message::Observe {
                snapshot,
                cursor_height,
                received_at_unix_ms,
                query_latency_ms,
                catching_up,
            } => {
                let observation = (|| {
                    let snapshot = self
                        .binding
                        .decode(&journal::canonical(&snapshot)?)
                        .map_err(invalid)?;
                    let obs = Observation {
                        snapshot_id: snapshot.id().into(),
                        cursor_height: codec::integer(&json!(cursor_height), 64).map_err(invalid)?
                            as u64,
                        received_at: codec::integer(&json!(received_at_unix_ms), 64)
                            .map_err(invalid)? as u64,
                        query_latency_ms: codec::integer(&json!(query_latency_ms), 64)
                            .map_err(invalid)? as u64,
                        catching_up,
                    };
                    self.service.observe(snapshot, obs, now, 0)?;
                    Ok((200, self.service.status(now)?))
                })();
                if observation.is_err() {
                    self.service.rpc_failed();
                }
                observation
            }
            Message::Request {
                method,
                path,
                origin,
                authorization,
                body_base64,
            } => {
                let body = request::bytes(&json!(body_base64)).map_err(invalid)?;
                if body.len() > request::MAX_REQUEST_BYTES || path.len() > 8192 {
                    return Err(Error::ResourceLimit);
                }
                self.route(
                    &method,
                    &path,
                    origin.as_deref(),
                    authorization.as_deref(),
                    &body,
                    now,
                )
            }
        }
    }
    fn error(&self, error: Error) -> Value {
        let code = match error {
            Error::Io(_) | Error::RecoveryRequired(_) => "RECOVERY_REQUIRED",
            Error::ResourceLimit => "RESOURCE_LIMIT",
            Error::WriterAlreadyRunning => "WRITER_ALREADY_RUNNING",
            Error::InvalidRecord("STALE" | "CLOCK_REGRESSION" | "FUTURE_BLOCK_TIME") => {
                "STALE_SNAPSHOT"
            }
            Error::InvalidRecord("RPC_UNAVAILABLE") => "SNAPSHOT_UNAVAILABLE",
            Error::InvalidRecord("OBSERVATION_REQUIRED") => "CATCHING_UP",
            Error::InvalidRecord("POISONED_SESSION" | "JOURNAL_FAILURE") => "RECOVERY_REQUIRED",
            Error::InvalidRecord(code) => code,
        };
        self.error_code(code)
    }
    fn error_code(&self, code: &str) -> Value {
        let table: Value = serde_json::from_str(include_str!("../../../protocol/s2/errors.json"))
            .expect("pinned errors");
        let found = table["new_errors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["code"] == code);
        let (status, retryable, state) = match found {
            Some(v) => (
                v["http_status"].clone(),
                v["retryable"].clone(),
                v["state"].clone(),
            ),
            None => (
                json!(if code == "INVALID_SIGNATURE" {
                    "401"
                } else {
                    "422"
                }),
                json!(false),
                json!("REJECTED"),
            ),
        };
        let private = matches!(code, "UNAUTHORIZED" | "FORBIDDEN" | "INVALID_SIGNATURE");
        json!({"http_status":status, "body":{"code":code, "retryable":retryable, "state":state,
            "observed_height":if private {Value::Null} else {json!(self.service.state().snapshot().height().to_string())},
            "stream_seq":if private {Value::Null} else {json!(self.service.state().sequence().to_string())}}})
    }
    #[allow(clippy::too_many_arguments)]
    fn route(
        &mut self,
        method: &str,
        path: &str,
        origin: Option<&str>,
        authorization: Option<&str>,
        body: &[u8],
        now: u64,
    ) -> journal::Result<(u16, Value)> {
        if method != "GET" && method != "POST" {
            return Err(invalid("UNSUPPORTED_METHOD"));
        }
        if method == "GET" && !body.is_empty() {
            return Err(invalid("NON_CANONICAL_WIRE"));
        }
        let (path, query) = parse_path(path).map_err(invalid)?;
        let public = method == "GET" && matches!(path, "/s2/network" | "/s2/status" | "/s2/book");
        if public {
            if !query.is_empty() {
                return Err(invalid("NON_CANONICAL_WIRE"));
            }
            return Ok((
                200,
                match path {
                    "/s2/book" => self.service.book()?,
                    "/s2/status" => self.service.status(now)?,
                    _ => {
                        json!({"context":self.binding.context(), "profile":"s2-local-v1", "gas_denom":"DEVGAS",
                    "assets":[{"denom":"DEVBASE","decimals":"6","atoms_per_unit":"1000000"},
                        {"denom":"DEVQUOTE","decimals":"6","atoms_per_unit":"1000000"}],
                    "origins":["http://127.0.0.1:5173","http://localhost:5173"], "status":self.service.status(now)?})
                    }
                },
            ));
        }
        if method == "POST" && !query.is_empty() {
            return Err(invalid("NON_CANONICAL_WIRE"));
        }
        if method == "POST" && path == "/s2/auth/challenges" {
            return Ok((
                200,
                self.auth
                    .challenge(body, origin, now / 1000)
                    .map_err(invalid)?,
            ));
        }
        if method == "POST" && path == "/s2/auth/sessions" {
            return Ok((
                200,
                self.auth
                    .session(body, origin, now / 1000)
                    .map_err(invalid)?,
            ));
        }
        let owner = self
            .auth
            .owner(authorization, origin, now / 1000)
            .map_err(invalid)?;
        let previous_seq = self.service.state().sequence();
        let result = match (method, path) {
            ("POST", "/s2/auth/logout") if body.is_empty() => {
                self.auth
                    .logout(authorization, origin, now / 1000)
                    .map_err(invalid)?;
                json!({"logged_out":true})
            }
            ("POST", "/s2/orders" | "/s2/cancels") => {
                let kind = if path == "/s2/orders" {
                    SignedKind::Order
                } else {
                    SignedKind::Cancel
                };
                let input =
                    SignedCommand::decode(kind, body, self.binding.context()).map_err(invalid)?;
                self.service.submit(
                    kind.command(),
                    &input.wire,
                    &input.signature,
                    &owner,
                    now,
                    0,
                )?
            }
            ("POST", "/s2/me/withdraw-prepare" | "/s2/me/withdraw-abort") => {
                let raw = request::local_action(body).map_err(invalid)?;
                self.service.submit(
                    if path.ends_with("prepare") {
                        "WITHDRAW_PREPARE"
                    } else {
                        "WITHDRAW_ABORT"
                    },
                    &raw,
                    &[],
                    &owner,
                    now,
                    0,
                )?
            }
            ("GET", "/s2/me") => {
                if query.keys().any(|k| k != "cursor") {
                    return Err(invalid("FORBIDDEN"));
                }
                match self
                    .service
                    .ledger_view(&owner, query.get("cursor").map(String::as_str), now)
                {
                    Err(Error::InvalidRecord("SNAPSHOT_CONFLICT")) => {
                        return Ok((409, self.error_code("SNAPSHOT_CONFLICT")["body"].clone()));
                    }
                    other => other?,
                }
            }
            ("GET", _) => self.lookup(path, &query, &owner)?,
            _ => return Err(invalid("UNSUPPORTED_ROUTE")),
        };
        // Policy rejections are durable receipts, not unrecorded transport errors.
        let status = if result["state"] == "REJECTED" {
            self.error_code(result["code"].as_str().unwrap_or("POLICY_REJECTED"))["http_status"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        } else if result["state"] == "LOCAL_ACCEPTED"
            && self.service.state().sequence() > previous_seq
        {
            201
        } else {
            200
        };
        Ok((status, result))
    }
    fn lookup(
        &self,
        path: &str,
        query: &BTreeMap<String, String>,
        owner: &str,
    ) -> journal::Result<Value> {
        let parts: Vec<_> = path.split('/').collect();
        let (kind, id, order) = match parts.as_slice() {
            ["", "s2", "me", "commands", kind, id] => (*kind, *id, false),
            ["", "s2", "me", "orders", id] => ("ORDER", *id, true),
            _ => return Err(invalid("UNSUPPORTED_ROUTE")),
        };
        if !matches!(
            kind,
            "ORDER" | "CANCEL" | "WITHDRAW_PREPARE" | "WITHDRAW_ABORT"
        ) || !is_hash(id)
            || query.keys().any(|k| k != "epoch")
        {
            return Err(invalid("NON_CANONICAL_WIRE"));
        }
        let epoch = query
            .get("epoch")
            .map(|e| codec::integer(&json!(e), 64).map(|n| n as u64))
            .transpose()
            .map_err(invalid)?;
        if kind == "ORDER" && epoch.is_none() {
            return Err(invalid("NON_CANONICAL_WIRE"));
        }
        let receipt = self
            .service
            .lookup(owner, kind, id, epoch)
            .ok_or(invalid("NOT_FOUND_AT_SEQ"))?;
        if !order {
            return Ok(receipt.clone());
        }
        let state = self.service.state().state_json("OPEN").map_err(invalid)?;
        let epoch_text = epoch.map(|e| e.to_string());
        let view = state["orders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| {
                o["owner"] == owner
                    && o["view"]["order_id"] == id
                    && epoch_text
                        .as_deref()
                        .is_some_and(|e| o["view"]["owner_epoch"] == e)
            })
            .map(|o| o["view"].clone())
            .unwrap_or(Value::Null);
        Ok(json!({"order":view,"receipt":receipt}))
    }
}
fn parse_path(path: &str) -> crate::Result<(&str, BTreeMap<String, String>)> {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    if !path.is_ascii() || path.contains(['%', '#']) {
        return Err("NON_CANONICAL_WIRE");
    }
    let mut values = BTreeMap::new();
    if !query.is_empty() {
        for part in query.split('&') {
            let (key, value) = part.split_once('=').ok_or("NON_CANONICAL_WIRE")?;
            let mut decoded = Vec::new();
            let mut bytes = value.bytes();
            while let Some(b) = bytes.next() {
                if b == b'%' {
                    let pair = [
                        bytes.next().ok_or("NON_CANONICAL_WIRE")?,
                        bytes.next().ok_or("NON_CANONICAL_WIRE")?,
                    ];
                    decoded.extend(hex::decode(pair).map_err(|_| "NON_CANONICAL_WIRE")?);
                } else {
                    decoded.push(b);
                }
            }
            let decoded = String::from_utf8(decoded).map_err(|_| "NON_CANONICAL_WIRE")?;
            if values.insert(key.to_owned(), decoded).is_some() {
                return Err("NON_CANONICAL_WIRE");
            }
        }
    }
    Ok((path, values))
}
/// Bounded before allocation and parsing. An oversized/partial frame terminates
/// the pipe; continuing could interpret a body suffix as a new control message.
pub fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut out = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if out.is_empty() {
                Ok(None)
            } else {
                Err(std::io::ErrorKind::UnexpectedEof.into())
            };
        }
        let newline = available.iter().position(|b| *b == b'\n');
        let count = newline.map_or(available.len(), |n| n + 1);
        if out.len() + count > MAX_CONTROL_BYTES + 1 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        out.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            out.pop();
            return Ok(Some(out));
        }
    }
}
pub fn serve(
    runtime: &mut Runtime,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    while let Some(raw) = read_message(reader)? {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| std::io::ErrorKind::InvalidData)?
            .as_millis()
            .try_into()
            .map_err(|_| std::io::ErrorKind::InvalidData)?;
        let response = runtime.handle(&raw, now);
        serde_json::to_writer(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

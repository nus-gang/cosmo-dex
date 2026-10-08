//! Direct REST handler component. No listener, signing keys or trusted-control route.
use super::super::{journal::canonical, snapshot::Snapshot};
use super::*;
use crate::s2::{
    auth::Auth,
    request::{self, SignedCommand, SignedKind},
};
use std::net::IpAddr;

/// In addition to Engine's validated runtime/home/profile binding, both explicit
/// component opt-ins must be present. A listener must bind this exact address.
pub struct Options {
    pub enabled: bool,
    pub acknowledge_unproven_space: bool,
    pub bind: IpAddr,
}
pub struct Request<'a> {
    pub peer: IpAddr,
    pub method: &'a str,
    pub path: &'a str,
    /// Preserve duplicate headers; the handler rejects ambiguous authentication.
    pub headers: &'a [(&'a str, &'a str)],
    pub body: &'a [u8],
}
pub struct Rest {
    engine: Arc<Engine>,
    auth: Mutex<Auth>,
    context: Value,
    snapshot: Snapshot,
}
fn select(v: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|k| v.get(*k).map(|v| ((*k).into(), v.clone())))
            .collect(),
    )
}
fn single<'a>(headers: &'a [(&str, &'a str)], name: &str) -> Result<Option<&'a str>> {
    let mut it = headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name));
    let v = it.next().map(|(_, v)| *v);
    if it.next().is_some() {
        return Err(Error::Invalid("DUPLICATE_HEADER"));
    }
    Ok(v)
}
impl Rest {
    pub fn new(engine: Arc<Engine>, snapshot: Snapshot, options: Options) -> Result<Self> {
        if !options.enabled || !options.acknowledge_unproven_space || !options.bind.is_loopback() {
            return Err(Error::Invalid("LOCAL_DEMO_DISABLED"));
        }
        let view = engine.reader().get()?;
        if view.state["context"] != *snapshot.context()
            || view.state["chain_snapshot"] != *snapshot.value()
        {
            return Err(Error::Invalid("CONTEXT_MISMATCH"));
        }
        Ok(Self {
            auth: Mutex::new(Auth::from_s3(&snapshot)),
            context: snapshot.context().clone(),
            snapshot,
            engine,
        })
    }
    /// Observation is provided by the trusted chain adapter, never browser input.
    /// now is Unix milliseconds; existing wallet auth uses Unix seconds.
    pub fn handle(&self, req: Request<'_>, o: &Observation, now: u64) -> (u16, Value) {
        match self.dispatch(req, o, now) {
            Ok(v) => (200, v),
            Err(Error::Invalid(code)) => (
                match code {
                    "UNAUTHORIZED" => 401,
                    "FORBIDDEN" => 403,
                    "NOT_FOUND" => 404,
                    "METHOD_NOT_ALLOWED" => 405,
                    "RESOURCE_LIMIT" => 413,
                    "RECEIPT_NOT_FOUND" => 404,
                    _ => 409,
                },
                json!({"code":code,"durable_ack":false}),
            ),
            // No paths, internal raw state or OS error details in public responses.
            Err(_) => (503, json!({"code":"RECOVERY_REQUIRED","durable_ack":false})),
        }
    }
    fn dispatch(&self, req: Request<'_>, o: &Observation, now: u64) -> Result<Value> {
        if !req.peer.is_loopback() {
            return Err(Error::Invalid("FORBIDDEN"));
        }
        if req.body.len() > request::MAX_REQUEST_BYTES
            || req.headers.len() > 32
            || req.headers.iter().any(|(k, v)| k.len() + v.len() > 4096)
        {
            return Err(Error::Invalid("RESOURCE_LIMIT"));
        }
        let path = req
            .path
            .strip_prefix("/dev-local/v1/")
            .ok_or(Error::Invalid("NOT_FOUND"))?;
        let origin = single(req.headers, "origin")?;
        request::mutation_origin(origin)?;
        let authorization = single(req.headers, "authorization")?;
        if req.method == "GET" && !req.body.is_empty() {
            return Err(Error::Invalid("NON_CANONICAL_WIRE"));
        }
        if ["auth/challenge", "auth/session"].contains(&path) {
            if req.method != "POST" {
                return Err(Error::Invalid("METHOD_NOT_ALLOWED"));
            }
            let mut auth = self
                .auth
                .lock()
                .map_err(|_| Error::Recovery("AUTH_POISONED"))?;
            return Ok(if path == "auth/challenge" {
                auth.challenge(req.body, origin, now / 1000)?
            } else {
                auth.session(req.body, origin, now / 1000)?
            });
        }
        let owner = self
            .auth
            .lock()
            .map_err(|_| Error::Recovery("AUTH_POISONED"))?
            .owner(authorization, origin, now / 1000)?;
        match (req.method, path) {
            ("POST", "auth/logout") => {
                self.auth
                    .lock()
                    .map_err(|_| Error::Recovery("AUTH_POISONED"))?
                    .logout(authorization, origin, now / 1000)?;
                Ok(json!({"logged_out":true}))
            }
            ("GET", "capabilities") => Ok(
                json!({"envelope_version":"s3-dev-local/1","profile_id":"s3-dev-local-v1","context":self.context,"api_prefix":"/dev-local/v1/","durable_ack":false,"storage_assurance":"UNPROVEN_HOST_SPACE","development_receipt":"LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE","public_receipt_version":crate::s3::dev_local::PUBLIC_RECEIPT_VERSION,"public_receipt_schema_sha256":crate::s3::dev_local::PUBLIC_RECEIPT_SCHEMA_SHA256,"trusted_receipt_version":"s3-dev-local/1","signed_result_query":true,"automatic_withdraw":false,"ws":false}),
            ),
            ("GET", "account") => self.account(&owner, o, now),
            ("POST", "orders" | "cancels" | "receipts/orders" | "receipts/cancels") => {
                let kind = if path.ends_with("orders") {
                    SignedKind::Order
                } else {
                    SignedKind::Cancel
                };
                let cmd = SignedCommand::decode(kind, req.body, &self.context)?;
                let result = if path.starts_with("receipts/") {
                    self.engine.query_signed(
                        kind.command(),
                        &cmd.wire,
                        &cmd.signature,
                        &owner,
                        o,
                        now,
                    )?
                } else {
                    self.engine.execute(
                        Command::Signed {
                            kind: kind.command().into(),
                            raw: cmd.wire,
                            signature: cmd.signature,
                            session_owner: owner.clone(),
                        },
                        &[],
                        o,
                        now,
                    )?
                };
                let result = result.ok_or(Error::Invalid("RECEIPT_NOT_FOUND"))?;
                self.public_receipt(
                    schema::num(&result["command_result"]["command_seq"])?,
                    &owner,
                )
            }
            ("POST", "withdraw/prepare" | "withdraw/abort") => {
                let body = request::object(req.body, &["context", "request_id"])?;
                if body["context"] != self.context {
                    return Err(Error::Invalid("CONTEXT_MISMATCH"));
                }
                let raw = request::local_action(
                    &serde_json::to_vec(&json!({"request_id":body["request_id"]}))
                        .map_err(|_| Error::Invalid("NON_CANONICAL_WIRE"))?,
                )?;
                let v = self
                    .engine
                    .execute(
                        Command::Local {
                            kind: if path.ends_with("prepare") {
                                "WITHDRAW_PREPARE"
                            } else {
                                "WITHDRAW_ABORT"
                            }
                            .into(),
                            raw,
                            session_owner: owner.clone(),
                        },
                        &[],
                        o,
                        now,
                    )?
                    .ok_or(Error::Invalid("NOT_FOUND"))?;
                self.public_receipt(schema::num(&v["command_result"]["command_seq"])?, &owner)
            }
            ("GET", path) if path.starts_with("receipts/commands/") => {
                let seq = &path["receipts/commands/".len()..];
                let seq =
                    schema::num(&json!(seq)).map_err(|_| Error::Invalid("NON_CANONICAL_WIRE"))?;
                if seq == 0 {
                    return Err(Error::Invalid("NON_CANONICAL_WIRE"));
                }
                self.public_receipt(seq, &owner)
            }
            _ => Err(Error::Invalid("NOT_FOUND")),
        }
    }
    fn public_receipt(&self, seq: u64, owner: &str) -> Result<Value> {
        self.engine
            .account_receipt(seq, owner)?
            .map(|r| r.to_value())
            .ok_or(Error::Invalid("RECEIPT_NOT_FOUND"))
    }
    /// Wire adapters must use these bytes verbatim (application/json, identity).
    /// The object handler remains for existing in-process component callers.
    pub fn handle_bytes(&self, req: Request<'_>, o: &Observation, now: u64) -> (u16, Vec<u8>) {
        let (status, value) = self.handle(req, o, now);
        match canonical(&value) {
            Ok(raw) => (status, raw),
            Err(_) => (
                503,
                br#"{"code":"RECOVERY_REQUIRED","durable_ack":false}"#.to_vec(),
            ),
        }
    }
    fn account(&self, owner: &str, o: &Observation, now: u64) -> Result<Value> {
        let v = self.engine.reader().get()?;
        let s = &v.state;
        let account = s["accounts"]
            .as_array()
            .ok_or("PROJECTION")?
            .iter()
            .find(|a| a["owner"] == owner)
            .ok_or("FORBIDDEN")?;
        let orders: Vec<_> = s["orders"]
            .as_array()
            .ok_or("PROJECTION")?
            .iter()
            .filter(|a| a["owner"] == owner)
            .map(|a| a["view"].clone())
            .collect();
        let owns = |hash: &Value| orders.iter().any(|a| a["order_hash"] == *hash);
        let fills: Vec<_> = s["fills"]
            .as_array()
            .ok_or("PROJECTION")?
            .iter()
            .filter(|f| owns(&f["buyer_order_hash"]) || owns(&f["seller_order_hash"]))
            .map(|f| {
                let mut out = select(
                    f,
                    &[
                        "fill_id",
                        "state",
                        "revision",
                        "reason",
                        "quantity_lots",
                        "execution_price_ticks",
                        "fee_base_atoms",
                        "fee_quote_atoms",
                    ],
                );
                out["batch"] = if f["batch"].is_null() {
                    Value::Null
                } else {
                    select(
                        &f["batch"],
                        &[
                            "batch_id",
                            "batch_hash",
                            "batch_seq",
                            "previous_batch_hash",
                            "operator_epoch",
                        ],
                    )
                };
                out
            })
            .collect();
        let batches: Vec<_> = s["batches"]
            .as_array()
            .ok_or("PROJECTION")?
            .iter()
            .filter(|b| {
                fills
                    .iter()
                    .any(|f| f["batch"]["batch_id"] == b["batch"]["batch_id"])
            })
            .map(|b| {
                let mut out = select(
                    b,
                    &[
                        "state",
                        "revision",
                        "observed_height",
                        "reason",
                        "attempt_hashes",
                    ],
                );
                out["batch"] = select(
                    &b["batch"],
                    &[
                        "batch_id",
                        "batch_hash",
                        "batch_seq",
                        "previous_batch_hash",
                        "operator_epoch",
                    ],
                );
                out["receipt"] = select(
                    &b["receipt"],
                    &[
                        "disposition",
                        "terminal_height",
                        "terminal_tx_hash",
                        "batch_receipt_v2",
                    ],
                );
                out
            })
            .collect();
        let fresh = self
            .snapshot
            .decode_related(&super::super::journal::canonical(&s["chain_snapshot"])?)
            .and_then(|s| s.freshness(o, now))
            .is_ok();
        let held = account["ledger"]
            .as_array()
            .ok_or("PROJECTION")?
            .iter()
            .any(|r| r["D"] != "0" || r["P"] != "0" || r["R"] != "0");
        Ok(
            json!({"context":self.context,"owner":owner,"revision":v.commit.command_seq.to_string(),"gate":v.gate,"durable_ack":false,"storage_assurance":"UNPROVEN_HOST_SPACE","fresh":fresh,"indexer_height":o.cursor_height.to_string(),"observed_height":s["chain_snapshot"]["height"],"snapshot_id":s["chain_snapshot"]["snapshot_id"],"received_at_unix_ms":o.received_at.to_string(),"query_latency_ms":o.query_latency_ms.to_string(),"ledger":account["ledger"],"withdraw_frozen":account["withdraw_frozen"],"withdraw_ready":fresh && v.gate == "OPEN" && !held && account["withdraw_frozen"] == true,"orders":orders,"fills":fills,"batches":batches}),
        )
    }
}

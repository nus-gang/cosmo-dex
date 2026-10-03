//! In-memory authentication for the approved loopback profile. No signing keys,
//! tokens or challenges are persisted. All operations run on the writer thread.
use super::{journal::sha256, request, snapshot::Snapshot};
use crate::{
    Result,
    codec::{self, Codec},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs::File, io::Read};

const MAX_ENTRIES: usize = 256;
struct Challenge {
    wire: Vec<u8>,
    expiry: u64,
}
struct Session {
    owner: String,
    origin: String,
    issued: u64,
    expiry: u64,
}
pub struct Auth {
    context: Value,
    keys: BTreeMap<String, Vec<u8>>,
    challenges: BTreeMap<String, Challenge>,
    sessions: BTreeMap<String, Session>,
    last_time: u64,
}
fn random() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| "AUTH_ENTROPY_UNAVAILABLE")?;
    Ok(bytes)
}
impl Auth {
    pub fn new(snapshot: &Snapshot) -> Self {
        Self {
            context: snapshot.value()["body"]["context"].clone(),
            keys: snapshot
                .accounts()
                .iter()
                .map(|a| (a.owner.clone(), a.public_key.clone()))
                .collect(),
            challenges: BTreeMap::new(),
            sessions: BTreeMap::new(),
            last_time: 0,
        }
    }
    fn clock(&mut self, now: u64) -> Result<()> {
        if now < self.last_time {
            self.challenges.clear();
            self.sessions.clear();
            return Err("UNAUTHORIZED");
        }
        self.last_time = now;
        self.challenges.retain(|_, c| now < c.expiry);
        self.sessions.retain(|_, s| now < s.expiry);
        Ok(())
    }
    pub fn challenge(&mut self, raw: &[u8], origin: Option<&str>, now: u64) -> Result<Value> {
        self.clock(now)?;
        let origin = request::mutation_origin(origin)?;
        let input = request::object(raw, &["owner", "origin", "audience"])?;
        let owner = input["owner"].as_str().ok_or("UNAUTHORIZED")?;
        if input["origin"] != origin
            || input["audience"] != "exchange-api"
            || !self.keys.contains_key(owner)
        {
            return Err("FORBIDDEN");
        }
        if self.challenges.len() >= MAX_ENTRIES {
            return Err("RESOURCE_LIMIT");
        }
        let expiry = now.checked_add(120).ok_or("INTEGER_OVERFLOW")?;
        let nonce = hex::encode(random()?);
        if self.challenges.contains_key(&nonce) {
            return Err("AUTH_ENTROPY_UNAVAILABLE");
        }
        let wire = Codec::default().encode(
            "WalletChallengeV1",
            &json!({
                "protocol_version":"1", "chain_id":self.context["chain_id"],
                "genesis_hash":self.context["genesis_hash"], "server_origin":origin,
                "audience":"exchange-api", "owner":owner, "challenge_nonce":nonce,
                "issued_at":now.to_string(), "expiry_time":expiry.to_string()
            }),
        )?;
        let response = json!({"wire_base64":STANDARD.encode(&wire)});
        self.challenges.insert(nonce, Challenge { wire, expiry });
        Ok(response)
    }
    /// Verify against the server-stored bytes and registered key, then consume
    /// the nonce and create a random bearer in one serialized mutation.
    pub fn session(&mut self, raw: &[u8], origin: Option<&str>, now: u64) -> Result<Value> {
        self.clock(now)?;
        let origin = request::mutation_origin(origin)?;
        let input = request::object(raw, &["wire_base64", "signature_base64"])?;
        let wire = request::bytes(&input["wire_base64"])?;
        let signature = request::bytes(&input["signature_base64"])?;
        let value = Codec::default().decode("WalletChallengeV1", &wire)?;
        let nonce = value["challenge_nonce"].as_str().ok_or("UNAUTHORIZED")?;
        let challenge = self.challenges.get(nonce).ok_or("UNAUTHORIZED")?;
        let owner = value["owner"].as_str().ok_or("UNAUTHORIZED")?;
        if challenge.wire != wire
            || value["server_origin"] != origin
            || value["audience"] != "exchange-api"
            || signature.len() != 3309
            || !codec::verify_raw(
                self.keys.get(owner).ok_or("UNAUTHORIZED")?,
                &codec::frame("NUS/WALLET_AUTH/V1", &wire),
                &signature,
                &[],
            )
        {
            return Err("UNAUTHORIZED");
        }
        if self.sessions.len() >= MAX_ENTRIES {
            return Err("RESOURCE_LIMIT");
        }
        let token = STANDARD.encode(random()?);
        let key = sha256(token.as_bytes());
        if self.sessions.contains_key(&key) {
            return Err("AUTH_ENTROPY_UNAVAILABLE");
        }
        let expiry = now.checked_add(300).ok_or("INTEGER_OVERFLOW")?;
        self.challenges.remove(nonce);
        self.sessions
            .retain(|_, s| s.owner != owner || s.origin != origin);
        self.sessions.insert(
            key,
            Session {
                owner: owner.into(),
                origin: origin.into(),
                issued: now,
                expiry,
            },
        );
        Ok(
            json!({"token":token, "owner":owner, "origin":origin, "audience":"exchange-api",
            "genesis_hash":self.context["genesis_hash"], "expiry_time":expiry.to_string()}),
        )
    }
    /// Origin is required on private reads too. The response never uses a body
    /// or URL owner as an authority. Tokens are hashed before in-memory lookup.
    pub fn owner(
        &mut self,
        authorization: Option<&str>,
        origin: Option<&str>,
        now: u64,
    ) -> Result<String> {
        self.clock(now)?;
        let origin = request::mutation_origin(origin)?;
        let token = authorization
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or("UNAUTHORIZED")?;
        if token.len() != 44 {
            return Err("UNAUTHORIZED");
        }
        let session = self
            .sessions
            .get(&sha256(token.as_bytes()))
            .ok_or("UNAUTHORIZED")?;
        if session.origin != origin || now < session.issued || now >= session.expiry {
            return Err("UNAUTHORIZED");
        }
        Ok(session.owner.clone())
    }
    pub fn logout(
        &mut self,
        authorization: Option<&str>,
        origin: Option<&str>,
        now: u64,
    ) -> Result<()> {
        self.owner(authorization, origin, now)?;
        let token = authorization.unwrap().strip_prefix("Bearer ").unwrap();
        self.sessions.remove(&sha256(token.as_bytes()));
        Ok(())
    }
}

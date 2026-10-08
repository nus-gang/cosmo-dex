//! Exact source bytes and an independently supplied runtime pin. A SHA proves
//! byte identity, never organizational approval. No component bypass exists.
use super::{Error, Result};
use crate::{
    codec,
    s3::{
        journal::{canonical, sha256},
        schema,
        snapshot::{Binding, Snapshot},
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const CANDIDATE: &str = "90169d322336a0c0de9bc6c48725d528d42fe74c78ea5b596fc7e059d747dda2";
const BASELINE: &str = "3ff69e73057a2bb6dcff64820123d520b9ad3e5637abbd1ad7d38b8c1a49eb97";
const PREFIX: &str = "proposals/s3-local-dev-v1/";
pub const PROFILE: &str = "s3-dev-local-v1";
#[derive(Clone)]
pub struct Inputs {
    pub approved_runtime_sha256: String,
    pub runtime_manifest: Vec<u8>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub effective_profile: Vec<u8>,
    pub guard: Vec<u8>,
    pub genesis: Vec<u8>,
    pub acknowledge_unproven_space: bool,
}
#[derive(Clone)]
pub struct Validated {
    pub(super) guard: Vec<u8>,
    pub(super) value: Value,
    pub(super) owners: Vec<Vec<u8>>,
    pub(super) bps: u32,
    pub(super) inputs: Inputs,
}
pub(super) fn exact(v: &Value, fields: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or(Error::Invalid("INPUT_FIELDS"))?;
    if o.len() != fields.len()
        || fields
            .iter()
            .any(|k| !o.contains_key(*k) || o[*k].is_null())
    {
        return Err(Error::Invalid("INPUT_FIELDS"));
    }
    Ok(())
}
fn object(raw: &[u8], fields: &[&str]) -> Result<Value> {
    if raw.is_empty() || raw.len() > 2 * 1024 * 1024 {
        return Err(Error::Invalid("INPUT_SIZE"));
    }
    let v = codec::unique_json(raw)?;
    exact(&v, fields)?;
    Ok(v)
}
fn hash(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn hashes(v: &Value) -> Result<BTreeMap<String, String>> {
    serde_json::from_value(v.clone()).map_err(|_| Error::Invalid("HASH_MAP"))
}
fn aggregate(m: &BTreeMap<String, String>) -> String {
    sha256(
        m.iter()
            .map(|(p, h)| format!("{h}  {p}\n"))
            .collect::<String>()
            .as_bytes(),
    )
}
impl Validated {
    pub fn new(incoming: Inputs) -> Result<Self> {
        let i = &incoming;
        if !i.acknowledge_unproven_space || i.effective_profile.is_empty() {
            return Err(Error::Invalid("LOCAL_DEMO_OPT_IN_REQUIRED"));
        }
        if !hash(&i.approved_runtime_sha256, 64)
            || sha256(&i.runtime_manifest) != i.approved_runtime_sha256
        {
            return Err(Error::Invalid("RUNTIME_MANIFEST_MISMATCH"));
        }
        let m = object(
            &i.runtime_manifest,
            &[
                "format",
                "scope",
                "candidate_manifest_sha256",
                "contract_sha256",
                "files_sha256",
                "components",
            ],
        )?;
        if m["format"] != "s3-dev-local-runtime/1"
            || m["scope"] != "REVIEWED_RUNTIME"
            || m["candidate_manifest_sha256"] != CANDIDATE
        {
            return Err(Error::Invalid("RUNTIME_MANIFEST_SCOPE"));
        }
        let files = hashes(&m["files_sha256"])?;
        let components = hashes(&m["components"])?;
        if files.len() > 512 || files.len() != i.files.len() || components.len() != 5 {
            return Err(Error::Invalid("RUNTIME_MANIFEST_FILES"));
        }
        let mut total = 0usize;
        for (name, digest) in &files {
            if name.is_empty()
                || name
                    .split('/')
                    .any(|c| c.is_empty() || c == "." || c == "..")
                || name.contains(['\\', '\0', '\r', '\n'])
                || !hash(digest, 64)
            {
                return Err(Error::Invalid("RUNTIME_MANIFEST_FILES"));
            }
            let raw = i
                .files
                .get(name)
                .ok_or(Error::Invalid("RUNTIME_FILE_MISSING"))?;
            total = total
                .checked_add(raw.len())
                .ok_or(Error::Invalid("INPUT_SIZE"))?;
            if raw.len() > 16 * 1024 * 1024 || total > 32 * 1024 * 1024 || sha256(raw) != *digest {
                return Err(Error::Invalid("RUNTIME_FILE_MISMATCH"));
            }
        }
        if m["contract_sha256"] != aggregate(&files) {
            return Err(Error::Invalid("CONTRACT_HASH_MISMATCH"));
        }
        let candidate = i
            .files
            .get(&format!("{PREFIX}MANIFEST.json"))
            .ok_or(Error::Invalid("INHERITED_MANIFEST_MISSING"))?;
        let baseline = i
            .files
            .get("protocol/s3/manifest.json")
            .ok_or(Error::Invalid("INHERITED_MANIFEST_MISSING"))?;
        if sha256(candidate) != CANDIDATE || sha256(baseline) != BASELINE {
            return Err(Error::Invalid("INHERITED_MANIFEST_MISMATCH"));
        }
        let c = codec::unique_json(candidate)?;
        let b = codec::unique_json(baseline)?;
        let mut expected = hashes(&b["files_sha256"])?;
        if b["contract_sha256"] != aggregate(&expected) {
            return Err(Error::Invalid("INHERITED_MANIFEST_MISMATCH"));
        }
        for (p, h) in hashes(&c["files_sha256"])? {
            expected.insert(format!("{PREFIX}{p}"), h);
        }
        expected.insert(format!("{PREFIX}MANIFEST.json"), CANDIDATE.into());
        expected.insert("protocol/s3/manifest.json".into(), BASELINE.into());
        // Approved public overlay is mandatory: old runtime/home inputs do not
        // silently acquire the new receipt decoder. The overlay manifest is a
        // pinned build input, excluded from its own runtime aggregate.
        let overlay: Value = serde_json::from_str(include_str!(
            "../../../../proposals/s3-local-account-receipt-v1/MANIFEST.json"
        ))
        .map_err(|_| Error::Invalid("ACCOUNT_CONTRACT_MANIFEST"))?;
        for (path, digest) in hashes(&overlay["files_sha256"])? {
            expected.insert(path, digest);
        }
        for name in ["chain", "exchange", "settlement", "wallet", "sre"] {
            let path = format!("chain/local-demo/components/{name}.json");
            if components.get(name) != Some(&path) {
                return Err(Error::Invalid("COMPONENT_BINDING_MISMATCH"));
            }
            let raw = i
                .files
                .get(&path)
                .ok_or(Error::Invalid("COMPONENT_MISSING"))?;
            let d = object(raw, &["head", "tree", "implementation_settings"])?;
            let settings = hashes(&d["implementation_settings"])?;
            if !hash(d["head"].as_str().unwrap_or(""), 40)
                || !hash(d["tree"].as_str().unwrap_or(""), 40)
                || settings.is_empty()
            {
                return Err(Error::Invalid("COMPONENT_BINDING_MISMATCH"));
            }
            expected.insert(path, sha256(raw));
        }
        if expected != files {
            return Err(Error::Invalid("INHERITED_FILE_SET_MISMATCH"));
        }
        let g = object(
            &i.guard,
            &[
                "envelope_version",
                "profile_id",
                "candidate_manifest_sha256",
                "runtime_manifest_sha256",
                "effective_profile_sha256",
                "run_uuid",
                "fee_profile",
                "context",
            ],
        )?;
        if canonical(&g)? != i.guard {
            return Err(Error::Invalid("NON_CANONICAL_GUARD"));
        }
        let uuid = g["run_uuid"].as_str().unwrap_or("");
        let pieces: Vec<_> = uuid.split('-').collect();
        if pieces.len() != 5
            || pieces
                .iter()
                .zip([8, 4, 4, 4, 12])
                .any(|(s, l)| !hash(s, l))
            || g["envelope_version"] != "s3-dev-local/1"
            || g["profile_id"] != PROFILE
            || g["candidate_manifest_sha256"] != CANDIDATE
            || g["runtime_manifest_sha256"] != i.approved_runtime_sha256
            || g["effective_profile_sha256"] != sha256(&i.effective_profile)
        {
            return Err(Error::Invalid("GUARD_MISMATCH"));
        }
        let fee = match g["fee_profile"].as_str() {
            Some("fee0") => 0,
            Some("fee25") => 25,
            _ => return Err(Error::Invalid("INVALID_FEE_PROFILE")),
        };
        if i.files
            .get(&format!("{PREFIX}effective-profile-fee{fee}.json"))
            != Some(&i.effective_profile)
        {
            return Err(Error::Invalid("EFFECTIVE_PROFILE_MISMATCH"));
        }
        if i.genesis.is_empty() || i.genesis.len() > 1024 * 1024 {
            return Err(Error::Invalid("GENESIS_SIZE"));
        }
        let genesis = codec::unique_json(&i.genesis)?;
        let app = &genesis["app_state"];
        exact(
            app,
            &[
                "public_keys",
                "settlement_operator_public_keys",
                "admin_public_key",
                "fee_bps",
                "contract_hash",
                "config_hash",
            ],
        )?;
        if genesis["chain_id"] != "nus-s3-dev-1"
            || genesis["initial_height"] != "1"
            || genesis["validators"]
                .as_array()
                .is_none_or(|v| v.len() != 4)
            || genesis["consensus_params"]["block"]["max_bytes"] != "1048576"
            || genesis["consensus_params"]["block"]["max_gas"] != "20000000"
            || genesis["consensus_params"]["evidence"]["max_bytes"] != "65536"
            || app["contract_hash"] != m["contract_sha256"]
            || app["config_hash"] != sha256(&i.effective_profile)
            || app["fee_bps"] != fee.to_string()
        {
            return Err(Error::Invalid("GENESIS_BINDING_MISMATCH"));
        }
        let mut validators = BTreeSet::new();
        for validator in genesis["validators"].as_array().unwrap() {
            let key = schema::bytes(&validator["pub_key"]["value"])?;
            if validator["pub_key"]["type"] != "tendermint/PubKeyEd25519"
                || key.len() != 32
                || !validators.insert(key)
                || schema::num(&validator["power"])? == 0
            {
                return Err(Error::Invalid("GENESIS_VALIDATORS"));
            }
        }
        let context = json!({"service_schema":"s3/3","chain_id":"nus-s3-dev-1","genesis_hash":sha256(&i.genesis),"contract_hash":m["contract_sha256"],"config_hash":sha256(&i.effective_profile),"market_id":"DEVBASE/DEVQUOTE","market_config_version":"1"});
        schema::validate("Context", &g["context"])?;
        if g["context"] != context {
            return Err(Error::Invalid("CONTEXT_MISMATCH"));
        }
        let users = app["public_keys"]
            .as_array()
            .ok_or(Error::Invalid("GENESIS_KEYS"))?;
        let operators = app["settlement_operator_public_keys"]
            .as_array()
            .ok_or(Error::Invalid("GENESIS_KEYS"))?;
        if !(2..=16).contains(&users.len()) || operators.len() != 2 {
            return Err(Error::Invalid("GENESIS_KEYS"));
        }
        let mut seen = BTreeSet::new();
        let mut owners = vec![];
        for (ix, key) in users
            .iter()
            .chain(operators)
            .chain(std::iter::once(&app["admin_public_key"]))
            .enumerate()
        {
            let raw = schema::bytes(key)?;
            if raw.len() != 1952 {
                return Err(Error::Invalid("GENESIS_KEYS"));
            }
            let owner =
                hex::decode(&sha256(&raw)[..40]).map_err(|_| Error::Invalid("GENESIS_KEYS"))?;
            if !seen.insert(owner.clone()) {
                return Err(Error::Invalid("DUPLICATE_ACCOUNT"));
            }
            if ix < users.len() {
                owners.push(owner);
            }
        }
        owners.sort();
        Ok(Self {
            guard: i.guard.clone(),
            value: g,
            owners,
            bps: fee,
            inputs: incoming,
        })
    }
    pub fn context(&self) -> &Value {
        &self.value["context"]
    }
    pub fn guard(&self) -> &[u8] {
        &self.guard
    }
    /// The bootstrap is a trusted same-height RPC snapshot, not arbitrary EngineState.
    /// Its exact bytes and this binding are persisted once and replayed thereafter.
    pub(super) fn bootstrap(&self, raw: &[u8]) -> Result<Snapshot> {
        let v = schema::decode("ChainSnapshot", raw)?;
        let supplies = [
            codec::integer(&v["assets"][0]["supply_atoms"], 128)?,
            codec::integer(&v["assets"][1]["supply_atoms"], 128)?,
        ];
        let binding = Binding::new(
            self.context().clone(),
            self.owners.clone(),
            supplies,
            self.bps,
        )?;
        let snapshot = binding.decode(raw)?;
        if snapshot.last_seq() != 0
            || v["terminal_batch_seqs"] != json!([])
            || v["owner_events"] != json!([])
        {
            return Err(Error::Invalid("BOOTSTRAP_NOT_FRESH"));
        }
        Ok(snapshot)
    }
    /// Private transport codec used only by the component binary. Pin comes from
    /// a separate CLI argument, never from this untrusted input bundle.
    pub fn decode_bundle(
        raw: &[u8],
        profile: Vec<u8>,
        pin: String,
        acknowledge: bool,
    ) -> Result<Self> {
        if raw.len() > 48 * 1024 * 1024 {
            return Err(Error::Invalid("INPUT_SIZE"));
        }
        let v = codec::unique_json(raw)?;
        exact(&v, &["runtime_manifest", "files", "guard", "genesis"])?;
        let decode = |v: &Value| {
            STANDARD
                .decode(v.as_str().ok_or(Error::Invalid("BASE64"))?)
                .map_err(|_| Error::Invalid("BASE64"))
        };
        let mut files = BTreeMap::new();
        for (k, v) in v["files"].as_object().ok_or(Error::Invalid("FILES"))? {
            files.insert(k.clone(), decode(v)?);
        }
        Self::new(Inputs {
            approved_runtime_sha256: pin,
            runtime_manifest: decode(&v["runtime_manifest"])?,
            files,
            effective_profile: profile,
            guard: decode(&v["guard"])?,
            genesis: decode(&v["genesis"])?,
            acknowledge_unproven_space: acknowledge,
        })
    }
}

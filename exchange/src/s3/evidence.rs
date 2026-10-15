//! Exact, immutable rc3 evidence objects. This in-memory set is a private input
//! cache, not a durability boundary. Disk publication belongs to the journal.
use super::{
    journal::{canonical, sha256},
    schema,
};
use crate::{Result, codec};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub const RPC: &str = "application/json";
pub const TX: &str = "application/vnd.nus.txraw";
pub const TYPED: &str = "application/vnd.nus.s3+json";
pub fn limit(media: &str) -> Result<usize> {
    match media {
        RPC => Ok(16_777_216),
        TX => Ok(139_264),
        TYPED => Ok(262_144),
        _ => Err("EVIDENCE_TYPE"),
    }
}
pub fn reference(raw: &[u8], media: &str) -> Result<Value> {
    if raw.is_empty() || raw.len() > limit(media)? {
        return Err("EVIDENCE_SIZE");
    }
    Ok(json!({"sha256":sha256(raw),"byte_length":raw.len().to_string(),"media_type":media}))
}
pub fn verify(reference: &Value, raw: &[u8]) -> Result<()> {
    schema::validate("EvidenceRef", reference)?;
    let media = reference["media_type"].as_str().ok_or("EVIDENCE_TYPE")?;
    if *reference != self::reference(raw, media)? {
        return Err("EVIDENCE_MISMATCH");
    }
    if media == RPC || media == TYPED {
        let value = codec::unique_json(raw)?;
        if media == TYPED && canonical(&value).map_err(|_| "EVIDENCE_CANONICAL")? != raw {
            return Err("EVIDENCE_CANONICAL");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct Objects(BTreeMap<String, (Value, Arc<[u8]>)>);
impl Objects {
    pub fn insert(&mut self, raw: &[u8], media: &str) -> Result<Value> {
        let r = reference(raw, media)?;
        verify(&r, raw)?;
        let hash = r["sha256"].as_str().unwrap();
        if let Some((old, bytes)) = self.0.get(hash) {
            if old != &r || bytes.as_ref() != raw {
                return Err("EVIDENCE_REF_CONFLICT");
            }
        } else {
            self.0.insert(hash.into(), (r.clone(), Arc::from(raw)));
        }
        Ok(r)
    }
    pub fn insert_typed(&mut self, name: &str, value: &Value) -> Result<Value> {
        schema::validate(name, value)?;
        self.insert(&canonical(value).map_err(|_| "EVIDENCE_CANONICAL")?, TYPED)
    }
    /// Every use rechecks bytes, length, hash, role and JSON; no trusted hash cache.
    pub fn resolve(&self, r: &Value, media: &str) -> Result<&[u8]> {
        if r["media_type"] != media {
            return Err("EVIDENCE_ROLE");
        }
        let (stored, raw) = self
            .0
            .get(r["sha256"].as_str().ok_or("EVIDENCE_REF")?)
            .ok_or("EVIDENCE_MISSING")?;
        if stored != r {
            return Err("EVIDENCE_REF_CONFLICT");
        }
        verify(r, raw)?;
        Ok(raw)
    }
    pub fn typed(&self, r: &Value, name: &str) -> Result<Value> {
        let raw = self.resolve(r, TYPED)?;
        schema::decode(name, raw)
    }
    pub fn entries(&self) -> impl Iterator<Item = (&Value, &[u8])> {
        self.0.values().map(|(r, raw)| (r, raw.as_ref()))
    }
    /// Slots determine exact metadata types. Arbitrary JSON cannot be adopted
    /// as an Attempt, ResolutionEvidence, or the latest ChainSnapshot.
    pub fn graph(&self, value: &Value) -> Result<Vec<Value>> {
        let mut seen = BTreeMap::new();
        let mut active = BTreeSet::new();
        self.walk(value, "", &mut seen, &mut active)?;
        Ok(seen.into_values().collect())
    }
    fn walk(
        &self,
        value: &Value,
        slot: &str,
        seen: &mut BTreeMap<String, Value>,
        active: &mut BTreeSet<String>,
    ) -> Result<()> {
        match value {
            Value::Object(o)
                if o.contains_key("sha256")
                    && o.contains_key("byte_length")
                    && o.contains_key("media_type") =>
            {
                schema::validate("EvidenceRef", value)?;
                let hash = value["sha256"].as_str().unwrap().to_owned();
                if active.contains(&hash) {
                    return Err("EVIDENCE_CYCLE");
                }
                if seen.get(&hash).is_some_and(|r| r != value) {
                    return Err("EVIDENCE_REF_CONFLICT");
                }
                // Validate the slot even if this object was already seen elsewhere.
                let typed = match slot {
                    "raw_tx_ref" => {
                        self.resolve(value, TX)?;
                        None
                    }
                    "raw_block_response_ref" | "raw_results_response_ref" => {
                        self.resolve(value, RPC)?;
                        None
                    }
                    "attempt_refs" => Some(self.typed(value, "Attempt")?),
                    "resolution_evidence_ref" => Some(self.typed(value, "ResolutionEvidence")?),
                    "latest_observation_ref" => Some(self.typed(value, "ChainSnapshot")?),
                    _ => return Err("EVIDENCE_ROLE"),
                };
                if !seen.contains_key(&hash) {
                    active.insert(hash.clone());
                    if let Some(v) = typed {
                        self.walk(&v, "", seen, active)?;
                    }
                    active.remove(&hash);
                    seen.insert(hash, value.clone());
                }
            }
            Value::Object(o) => {
                for (key, v) in o {
                    self.walk(v, key, seen, active)?;
                }
            }
            Value::Array(a) => {
                for v in a {
                    self.walk(v, slot, seen, active)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn verify_exact_refs(&self, value: &Value, refs: &Value) -> Result<()> {
        if json!(self.graph(value)?) != *refs {
            return Err("EVIDENCE_REF_SET");
        }
        Ok(())
    }
}

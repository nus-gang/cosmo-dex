use super::{Error, Result, Validated, fault, fs::Root};
use crate::s3::{
    evidence::{self, Objects},
    journal::{Commit, HEADER, MAX_PAYLOAD, canonical, sha256},
    schema,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::DirBuilderExt,
    path::Path,
};
pub(super) fn frame(raw: &[u8]) -> Result<Vec<u8>> {
    if raw.len() > MAX_PAYLOAD {
        return Err(Error::Invalid("STORAGE_CAPACITY"));
    }
    let mut b = Vec::with_capacity(HEADER + raw.len());
    b.extend_from_slice(b"S3D1");
    b.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    b.extend_from_slice(&Sha256::digest(raw));
    b.extend_from_slice(&Sha256::digest(&b));
    b.extend_from_slice(raw);
    Ok(b)
}
fn read_frame(f: &mut File) -> Result<Option<(Value, String, u64)>> {
    let mut h = [0u8; HEADER];
    if f.read(&mut h[..1])? == 0 {
        return Ok(None);
    }
    f.read_exact(&mut h[1..])
        .map_err(|_| Error::Recovery("PARTIAL_HEADER"))?;
    if &h[..4] != b"S3D1" || Sha256::digest(&h[..40])[..] != h[40..] {
        return Err(Error::Recovery("MAGIC_HEADER_CHECKSUM"));
    }
    let n = u32::from_be_bytes(h[4..8].try_into().unwrap()) as usize;
    if n > MAX_PAYLOAD {
        return Err(Error::Recovery("PAYLOAD_LIMIT"));
    }
    let mut raw = vec![0; n];
    f.read_exact(&mut raw)
        .map_err(|_| Error::Recovery("PARTIAL_PAYLOAD"))?;
    if Sha256::digest(&raw)[..] != h[8..40] {
        return Err(Error::Recovery("PAYLOAD_CHECKSUM"));
    }
    let v = crate::codec::unique_json(&raw)?;
    if canonical(&v)? != raw {
        return Err(Error::Recovery("CANONICAL_RECORD"));
    }
    let mut all = h.to_vec();
    all.extend(raw);
    Ok(Some((v, sha256(&all), (HEADER + n) as u64)))
}
fn commit_value(c: &Commit) -> Value {
    json!({"command_seq":c.command_seq.to_string(),"record_hash":c.record_hash,"end_offset":c.end_offset.to_string()})
}
struct Lock(File);
impl Lock {
    fn new(f: File) -> Result<Self> {
        f.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => Error::WriterAlreadyRunning,
            std::fs::TryLockError::Error(e) => e.into(),
        })?;
        Ok(Self(f))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub(super) struct Store {
    root: Root,
    wal: File,
    config: Validated,
    pub commit: Commit,
    poisoned: bool,
    _lock: Lock,
}
const FILES: &[&str] = &[
    "bootstrap.dev.json",
    "commit.dev.json",
    "effective.dev.json",
    "genesis.dev.json",
    "home.dev.json",
    "journal.dev.wal",
    "objects",
    "profile.guard.json",
    "runtime.dev.json",
    "writer.dev.lock",
];
impl Store {
    fn namespace(path: &Path, c: &Validated) -> Result<()> {
        let suffix = format!(
            ".runtime/s3-dev-local-v1/{}/{}",
            c.value["run_uuid"].as_str().unwrap(),
            c.value["fee_profile"].as_str().unwrap()
        );
        if !path.is_absolute()
            || !path.ends_with(suffix)
            || path.as_os_str()
                != fs::canonicalize(path.parent().ok_or(Error::Invalid("ROOT_PARENT"))?)?
                    .join(path.file_name().ok_or(Error::Invalid("ROOT_NAME"))?)
                    .as_os_str()
        {
            return Err(Error::Recovery("DEV_HOME_NAMESPACE"));
        }
        Ok(())
    }
    pub fn create(path: &Path, c: Validated, bootstrap: &[u8]) -> Result<Self> {
        Self::namespace(path, &c)?;
        c.bootstrap(bootstrap)?;
        fs::DirBuilder::new().mode(0o700).create(path)?;
        File::open(path.parent().unwrap())?.sync_all()?;
        let root = Root::open(path)?;
        let lock = Lock::new(root.dir.file("writer.dev.lock", true)?)?;
        // Only ownership lock exists before the immutable guard reaches disk.
        root.dir.write_new("profile.guard.json", c.guard())?;
        fault("guard_complete")?;
        root.dir.write_new("home.dev.json",&canonical(&json!({"identity":root.identity,"guard_sha256":sha256(c.guard()),"bootstrap_sha256":sha256(bootstrap)}))?)?;
        root.dir
            .write_new("runtime.dev.json", &c.inputs.runtime_manifest)?;
        root.dir.write_new("genesis.dev.json", &c.inputs.genesis)?;
        root.dir
            .write_new("effective.dev.json", &c.inputs.effective_profile)?;
        root.dir.write_new("bootstrap.dev.json", bootstrap)?;
        root.dir.child("objects", true)?.child("sha256", true)?;
        let wal = root.dir.file("journal.dev.wal", true)?;
        wal.sync_all()?;
        let s = Self {
            root,
            wal,
            config: c,
            commit: Commit::zero(),
            poisoned: false,
            _lock: lock,
        };
        s.root
            .dir
            .write_new("commit.dev.json", &canonical(&commit_value(&s.commit))?)?;
        s.check()?;
        Ok(s)
    }
    pub fn open(path: &Path, c: Validated) -> Result<(Self, Vec<u8>, Vec<Value>)> {
        Self::namespace(path, &c)?;
        let root = Root::open(path)?;
        if root.dir.read("profile.guard.json", 4096)? != c.guard() {
            return Err(Error::Recovery("GUARD_MISMATCH"));
        }
        let lock = Lock::new(root.dir.file("writer.dev.lock", false)?)?;
        let wal = root.dir.file("journal.dev.wal", false)?;
        let mut s = Self {
            root,
            wal,
            config: c,
            commit: Commit::zero(),
            poisoned: false,
            _lock: lock,
        };
        let bootstrap = s.check_files()?;
        let raw = s.root.dir.read("commit.dev.json", 4096)?;
        let v = crate::codec::unique_json(&raw)?;
        let expected = Commit {
            command_seq: schema::num(&v["command_seq"])?,
            record_hash: v["record_hash"]
                .as_str()
                .ok_or(Error::Recovery("MARKER_HASH"))?
                .into(),
            end_offset: schema::num(&v["end_offset"])?,
        };
        if canonical(&commit_value(&expected))? != raw {
            return Err(Error::Recovery("MARKER_FIELDS"));
        }
        let mut records = vec![];
        while let Some((r, h, n)) = read_frame(&mut s.wal)? {
            if r["context"] != *s.config.context()
                || schema::num(&r["command_seq"])?
                    != s.commit
                        .command_seq
                        .checked_add(1)
                        .ok_or(Error::Recovery("SEQUENCE_OVERFLOW"))?
                || r["previous_commit_hash"] != s.commit.record_hash
            {
                return Err(Error::Recovery("CHAIN_MISMATCH"));
            }
            let end = s
                .commit
                .end_offset
                .checked_add(n)
                .ok_or(Error::Recovery("OFFSET_OVERFLOW"))?;
            if end > expected.end_offset {
                return Err(Error::Recovery("UNKNOWN_TAIL"));
            }
            schema::validate("JournalRecord", &r)?;
            s.load(&r["evidence_refs"])?;
            s.commit = Commit {
                command_seq: schema::num(&r["command_seq"])?,
                record_hash: h,
                end_offset: end,
            };
            records.push(r);
        }
        if s.commit != expected {
            return Err(Error::Recovery("COMMITTED_FRAME_MISSING"));
        }
        s.check()?;
        Ok((s, bootstrap, records))
    }
    pub fn check(&self) -> Result<Vec<u8>> {
        let bootstrap = self.check_files()?;
        if self.root.dir.read("commit.dev.json", 4096)? != canonical(&commit_value(&self.commit))?
            || self.wal.metadata()?.len() != self.commit.end_offset
        {
            return Err(Error::Recovery("COMMITTED_STORE_CHANGED"));
        }
        Ok(bootstrap)
    }
    fn check_files(&self) -> Result<Vec<u8>> {
        if self.poisoned {
            return Err(Error::Recovery("POISONED_WRITER"));
        }
        self.root.check()?;
        let names = self.root.dir.names()?;
        if names != FILES.iter().map(|s| s.to_string()).collect::<Vec<_>>() {
            return Err(Error::Recovery("UNKNOWN_OR_INCOMPLETE_STORE"));
        }
        self.root.dir.check_file("writer.dev.lock", &self._lock.0)?;
        self.root.dir.check_file("journal.dev.wal", &self.wal)?;
        for (name, want) in [
            ("profile.guard.json", &self.config.guard),
            ("runtime.dev.json", &self.config.inputs.runtime_manifest),
            ("genesis.dev.json", &self.config.inputs.genesis),
            ("effective.dev.json", &self.config.inputs.effective_profile),
        ] {
            if self.root.dir.read(name, want.len())? != *want {
                return Err(Error::Recovery("INPUT_BYTES_CHANGED"));
            }
        }
        let bootstrap = self.root.dir.read("bootstrap.dev.json", 262144)?;
        self.config.bootstrap(&bootstrap)?;
        let home = canonical(
            &json!({"identity":self.root.identity,"guard_sha256":sha256(self.config.guard()),"bootstrap_sha256":sha256(&bootstrap)}),
        )?;
        if self.root.dir.read("home.dev.json", home.len())? != home {
            return Err(Error::Recovery("HOME_IDENTITY"));
        }
        let objects = self.root.dir.child("objects", false)?;
        if objects.names()? != vec!["sha256".to_owned()] {
            return Err(Error::Recovery("UNKNOWN_OBJECT_DIRECTORY"));
        }
        let objects = objects.child("sha256", false)?;
        let names = objects.names()?;
        for n in &names {
            let h = n.strip_suffix(".ref").unwrap_or(n);
            if h.len() != 64
                || !h
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !names.contains(&h.to_owned())
                || !names.contains(&format!("{h}.ref"))
            {
                return Err(Error::Recovery("INCOMPLETE_EVIDENCE"));
            }
            if n.ends_with(".ref") {
                let r = crate::codec::unique_json(&objects.read(n, 4096)?)?;
                if r["sha256"] != h {
                    return Err(Error::Recovery("EVIDENCE_NAME"));
                }
                self.read(&r)?;
            }
        }
        self.root.check()?;
        Ok(bootstrap)
    }
    pub fn read(&self, r: &Value) -> Result<Vec<u8>> {
        schema::validate("EvidenceRef", r)?;
        let hash = r["sha256"].as_str().unwrap();
        let media = r["media_type"].as_str().unwrap();
        let dir = self
            .root
            .dir
            .child("objects", false)?
            .child("sha256", false)?;
        if dir.read(&format!("{hash}.ref"), 4096)? != canonical(r)? {
            return Err(Error::Recovery("EVIDENCE_DESCRIPTOR"));
        }
        let raw = dir.read(hash, evidence::limit(media)?)?;
        evidence::verify(r, &raw)?;
        Ok(raw)
    }
    pub fn load(&self, refs: &Value) -> Result<Objects> {
        let mut objects = Objects::default();
        for r in refs.as_array().ok_or(Error::Recovery("EVIDENCE_REFS"))? {
            objects.insert(&self.read(r)?, r["media_type"].as_str().unwrap())?;
        }
        Ok(objects)
    }
    pub fn begin(&mut self, objects: &Objects) -> Result<()> {
        self.check()?;
        self.poisoned = true;
        self.root
            .dir
            .write_new("transaction.dev", &canonical(&commit_value(&self.commit))?)?;
        let dir = self
            .root
            .dir
            .child("objects", false)?
            .child("sha256", false)?;
        for (r, raw) in objects.entries() {
            let hash = r["sha256"].as_str().unwrap();
            evidence::verify(r, raw)?;
            if dir.names()?.contains(&hash.to_owned()) {
                self.read(r)?;
            } else {
                fault("evidence_write")?;
                dir.write_new(hash, raw)?;
                dir.write_new(&format!("{hash}.ref"), &canonical(r)?)?;
                self.read(r)?;
            }
        }
        fault("evidence_complete")?;
        Ok(())
    }
    pub fn append(&mut self, r: &Value) -> Result<Commit> {
        if !self.poisoned {
            return Err(Error::Recovery("TRANSACTION_REQUIRED"));
        }
        self.root.check()?;
        self.root.dir.check_file("journal.dev.wal", &self.wal)?;
        if self.wal.metadata()?.len() != self.commit.end_offset {
            return Err(Error::Recovery("WAL_CHANGED"));
        }
        if r["context"] != *self.config.context()
            || schema::num(&r["command_seq"])?
                != self
                    .commit
                    .command_seq
                    .checked_add(1)
                    .ok_or(Error::Invalid("SEQUENCE_OVERFLOW"))?
            || r["previous_commit_hash"] != self.commit.record_hash
        {
            return Err(Error::Recovery("CHAIN_MISMATCH"));
        }
        self.load(&r["evidence_refs"])?;
        let bytes = frame(&canonical(r)?)?;
        let next = Commit {
            command_seq: schema::num(&r["command_seq"])?,
            record_hash: sha256(&bytes),
            end_offset: self
                .commit
                .end_offset
                .checked_add(bytes.len() as u64)
                .ok_or(Error::Recovery("OFFSET_OVERFLOW"))?,
        };
        self.wal.seek(SeekFrom::Start(self.commit.end_offset))?;
        fault("before_wal")?;
        // The midpoint hook permits a real partial-write process exit.
        let mid = bytes.len() / 2;
        self.wal.write_all(&bytes[..mid])?;
        fault("partial_wal")?;
        self.wal.write_all(&bytes[mid..])?;
        fault("wal_sync")?;
        self.wal.sync_all()?;
        fault("after_wal_sync")?;
        let mut marker = self.root.dir.file("commit.dev.json.tmp", true)?;
        marker.write_all(&canonical(&commit_value(&next))?)?;
        fault("marker_sync")?;
        marker.sync_all()?;
        fault("after_marker_sync")?;
        self.root
            .dir
            .replace("commit.dev.json.tmp", "commit.dev.json")?;
        fault("after_marker_rename")?;
        fault("marker_dir_sync")?;
        self.root.dir.sync()?;
        fault("after_marker_dir_sync")?;
        self.root.check()?;
        self.root.dir.remove("transaction.dev")?;
        self.root.dir.sync()?;
        self.commit = next.clone();
        self.poisoned = false;
        self.check()?;
        fault("after_commit")?;
        Ok(next)
    }
}

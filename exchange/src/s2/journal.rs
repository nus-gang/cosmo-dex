//! Local single-writer journal. The sequencer owns semantic/schema validation.
//!
//! No receipt may be published until `append` returns. A returned IO error poisons
//! this handle: even an error after rename can mean UNKNOWN, never REJECTED.
//! Recovery stops on every uncertain tail; no truncation or empty-ledger fallback.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_PAYLOAD: usize = 16_777_216;
pub const HEADER: usize = 72;
const MARKER_ALLOWANCE: usize = 8192;
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    WriterAlreadyRunning,
    RecoveryRequired(&'static str),
    ResourceLimit,
    InvalidRecord(&'static str),
}
pub type Result<T> = std::result::Result<T, Error>;
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The S2 schema contains ASCII values and decimal strings, never JSON numbers.
/// BTreeMap-backed serde_json objects provide lexicographic ASCII key order.
pub fn canonical(value: &Value) -> Result<Vec<u8>> {
    fn check(v: &Value) -> bool {
        match v {
            Value::Null | Value::Bool(_) => true,
            Value::String(s) => s.is_ascii() && !s.contains(char::from(127)),
            Value::Array(a) => a.iter().all(check),
            Value::Object(o) => o.iter().all(|(k, v)| k.is_ascii() && check(v)),
            Value::Number(_) => false,
        }
    }
    if !check(value) {
        return Err(Error::InvalidRecord("NON_CANONICAL_VALUE"));
    }
    serde_json::to_vec(value).map_err(|_| Error::InvalidRecord("JSON"))
}

pub fn frame(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::ResourceLimit);
    }
    let mut out = Vec::with_capacity(HEADER + payload.len());
    out.extend_from_slice(b"S2W1");
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&Sha256::digest(payload));
    let header_hash = Sha256::digest(&out);
    out.extend_from_slice(&header_hash);
    out.extend_from_slice(payload);
    Ok(out)
}

fn read_frame(file: &mut File) -> Result<Option<(Value, String, u64)>> {
    let mut header = [0u8; HEADER];
    let n = file.read(&mut header[..1])?;
    if n == 0 {
        return Ok(None);
    }
    file.read_exact(&mut header[1..])
        .map_err(|_| Error::RecoveryRequired("PARTIAL_HEADER"))?;
    // Validate the entire header before trusting its length.
    if &header[..4] != b"S2W1" || Sha256::digest(&header[..40])[..] != header[40..] {
        return Err(Error::RecoveryRequired("HEADER_CHECKSUM"));
    }
    let len = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
    if len > MAX_PAYLOAD {
        return Err(Error::RecoveryRequired("PAYLOAD_LIMIT"));
    }
    let mut payload = vec![0; len];
    file.read_exact(&mut payload)
        .map_err(|_| Error::RecoveryRequired("PARTIAL_PAYLOAD"))?;
    if Sha256::digest(&payload)[..] != header[8..40] {
        return Err(Error::RecoveryRequired("PAYLOAD_CHECKSUM"));
    }
    let value: Value =
        serde_json::from_slice(&payload).map_err(|_| Error::RecoveryRequired("JSON"))?;
    // Re-encoding equality also rejects duplicate keys, whitespace and alternate escapes.
    if canonical(&value).map_err(|_| Error::RecoveryRequired("CANONICAL"))? != payload {
        return Err(Error::RecoveryRequired("CANONICAL"));
    }
    let mut hasher = Sha256::new();
    hasher.update(header);
    hasher.update(payload);
    Ok(Some((
        value,
        hex::encode(hasher.finalize()),
        (HEADER + len) as u64,
    )))
}
fn decimal(v: &Value, field: &str) -> Result<u64> {
    let s = v[field]
        .as_str()
        .ok_or(Error::InvalidRecord("INTEGER_TYPE"))?;
    let n: u64 = s
        .parse()
        .map_err(|_| Error::InvalidRecord("INTEGER_RANGE"))?;
    if n.to_string() != s {
        return Err(Error::InvalidRecord("INTEGER_CANONICAL"));
    }
    Ok(n)
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub command_seq: u64,
    pub record_hash: String,
    pub end_offset: u64,
}
impl Commit {
    fn zero() -> Self {
        Self {
            command_seq: 0,
            record_hash: ZERO_HASH.into(),
            end_offset: 0,
        }
    }
    fn value(&self) -> Value {
        json!({"command_seq": self.command_seq.to_string(), "record_hash": self.record_hash,
               "end_offset": self.end_offset.to_string()})
    }
}

/// Points used by the subprocess crash harness. Production always uses `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    BeforeAppend,
    AfterWalSync,
    AfterMarkerSync,
    AfterRename,
    AfterCommit,
}

pub struct Journal {
    dir: PathBuf,
    _lock: File,
    wal: File,
    context: Value,
    commit: Commit,
    poisoned: bool,
}
impl Journal {
    /// Explicit new namespace only. An existing or incomplete directory is never reused.
    pub fn create(path: &Path, context: Value) -> Result<Self> {
        canonical(&context)?;
        fs::create_dir(path)?;
        sync_dir(path.parent().ok_or(Error::InvalidRecord("PARENT"))?)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("writer.lock"))?;
        lock.try_lock().map_err(|_| Error::WriterAlreadyRunning)?;
        let wal = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("journal.wal"))?;
        wal.sync_all()?;
        let mut ctx = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join("context.json"))?;
        ctx.write_all(&canonical(&context)?)?;
        ctx.sync_all()?;
        let mut this = Self {
            dir: path.into(),
            _lock: lock,
            wal,
            context,
            commit: Commit::zero(),
            poisoned: false,
        };
        this.reserve()?;
        this.write_marker(&Commit::zero(), None)?;
        Ok(this)
    }

    /// Returns committed records only; the caller must replay/verify semantic hashes
    /// before publishing any recovered state. No external effects are sent here.
    pub fn open(path: &Path, context: Value) -> Result<(Self, Vec<Value>)> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("writer.lock"))?;
        lock.try_lock().map_err(|_| Error::WriterAlreadyRunning)?;
        let wal = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("journal.wal"))?;
        let mut this = Self {
            dir: path.into(),
            _lock: lock,
            wal,
            context,
            commit: Commit::zero(),
            poisoned: false,
        };
        match this.recover() {
            Ok(records) => {
                this.reserve()?;
                Ok((this, records))
            }
            Err(err) => {
                // Preserve the evidence even when the caller receives an error.
                // Copy failure is also fail-closed; original files are never modified.
                this.preserve_evidence()?;
                Err(err)
            }
        }
    }
    pub fn commit(&self) -> &Commit {
        &self.commit
    }
    fn recover(&mut self) -> Result<Vec<Value>> {
        if fs::read(self.dir.join("context.json"))? != canonical(&self.context)? {
            return Err(Error::RecoveryRequired("CONTEXT_MISMATCH"));
        }
        if self.dir.join("marker.tmp").exists() {
            return Err(Error::RecoveryRequired("UNCERTAIN_MARKER"));
        }
        let mut mf = File::open(self.dir.join("commit.marker"))?;
        let (marker, _, _) = read_frame(&mut mf)?.ok_or(Error::RecoveryRequired("EMPTY_MARKER"))?;
        if read_frame(&mut mf)?.is_some() {
            return Err(Error::RecoveryRequired("MARKER_TAIL"));
        }
        let expected = Commit {
            command_seq: decimal(&marker, "command_seq")?,
            record_hash: marker["record_hash"]
                .as_str()
                .ok_or(Error::RecoveryRequired("MARKER_HASH"))?
                .into(),
            end_offset: decimal(&marker, "end_offset")?,
        };
        if marker != expected.value() {
            return Err(Error::RecoveryRequired("MARKER_FIELDS"));
        }
        let mut records = Vec::new();
        while let Some((record, hash, bytes)) = read_frame(&mut self.wal)? {
            let seq = self
                .commit
                .command_seq
                .checked_add(1)
                .ok_or(Error::RecoveryRequired("SEQUENCE_OVERFLOW"))?;
            if decimal(&record, "command_seq")? != seq
                || record["previous_commit_hash"] != self.commit.record_hash
                || record["context"] != self.context
            {
                return Err(Error::RecoveryRequired("CHAIN_MISMATCH"));
            }
            self.commit = Commit {
                command_seq: seq,
                record_hash: hash,
                end_offset: self
                    .commit
                    .end_offset
                    .checked_add(bytes)
                    .ok_or(Error::RecoveryRequired("OFFSET_OVERFLOW"))?,
            };
            if self.commit.end_offset > expected.end_offset {
                return Err(Error::RecoveryRequired("UNKNOWN_TAIL"));
            }
            records.push(record);
        }
        if self.commit != expected {
            return Err(Error::RecoveryRequired("COMMITTED_FRAME_MISSING"));
        }
        Ok(records)
    }
    fn preserve_evidence(&self) -> Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::RecoveryRequired("CLOCK"))?
            .as_nanos();
        let target = self
            .dir
            .join(format!("evidence-{stamp}-{}", std::process::id()));
        fs::create_dir(&target)?;
        for name in [
            "journal.wal",
            "commit.marker",
            "marker.tmp",
            "context.json",
            "snapshot.json",
        ] {
            let src = self.dir.join(name);
            if src.exists() {
                fs::copy(src, target.join(name))?;
                File::open(target.join(name))?.sync_all()?;
            }
        }
        sync_dir(&target)?;
        sync_dir(&self.dir)
    }
    // Fixed maximum reserve is deliberately conservative: every admitted payload and
    // its caller-provided worst correction must fit MAX_PAYLOAD. Write real blocks,
    // not set_len (which could create a sparse file). Ordinary append cannot use it.
    fn reserve(&mut self) -> Result<()> {
        let path = self.dir.join("correction.reserve");
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let required = (MAX_PAYLOAD + HEADER + MARKER_ALLOWANCE) as u64;
        // Always rewrite after restart, since file length alone does not prove allocation.
        file.seek(SeekFrom::Start(0))?;
        let block = [0u8; 65536];
        let mut remaining = required;
        while remaining > 0 {
            let len = remaining.min(block.len() as u64) as usize;
            file.write_all(&block[..len])
                .map_err(|_| Error::ResourceLimit)?;
            remaining -= len as u64;
        }
        file.set_len(required)?;
        file.sync_all().map_err(|_| Error::ResourceLimit)?;
        sync_dir(&self.dir)
    }
    fn write_marker(&self, next: &Commit, crash: Option<CrashPoint>) -> Result<()> {
        let tmp = self.dir.join("marker.tmp");
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(&frame(&canonical(&next.value())?)?)?;
        file.sync_all()?;
        crash_at(crash, CrashPoint::AfterMarkerSync);
        fs::rename(tmp, self.dir.join("commit.marker"))?;
        crash_at(crash, CrashPoint::AfterRename);
        sync_dir(&self.dir)
    }

    /// Caller supplies the actual conservative serialized maximum correction size
    /// (CONTRACT §6), not an estimated record count. State stays private until Ok.
    /// Internal correction may consume the dedicated reserve; normal append may not.
    pub fn append(
        &mut self,
        record: &Value,
        maximum_correction_payload_bytes: usize,
        correction: bool,
    ) -> Result<Commit> {
        self.append_with_crash(record, maximum_correction_payload_bytes, correction, None)
    }
    pub fn append_with_crash(
        &mut self,
        record: &Value,
        maximum_correction_payload_bytes: usize,
        correction: bool,
        crash: Option<CrashPoint>,
    ) -> Result<Commit> {
        if self.poisoned {
            return Err(Error::RecoveryRequired("POISONED_WRITER"));
        }
        if correction != (record["command_kind"] == "CORRECTION") {
            return Err(Error::InvalidRecord("CORRECTION_RESERVE_ACCESS"));
        }
        if maximum_correction_payload_bytes > MAX_PAYLOAD {
            return Err(Error::ResourceLimit);
        }
        let seq = self
            .commit
            .command_seq
            .checked_add(1)
            .ok_or(Error::ResourceLimit)?;
        if decimal(record, "command_seq")? != seq
            || record["previous_commit_hash"] != self.commit.record_hash
            || record["context"] != self.context
        {
            return Err(Error::InvalidRecord("CHAIN_MISMATCH"));
        }
        let encoded = frame(&canonical(record)?)?;
        let next = Commit {
            command_seq: seq,
            record_hash: sha256(&encoded),
            end_offset: self
                .commit
                .end_offset
                .checked_add(encoded.len() as u64)
                .ok_or(Error::ResourceLimit)?,
        };
        // Catch external truncation or extra tail before appending. No silent overwrite.
        if self.wal.metadata()?.len() != self.commit.end_offset {
            self.poisoned = true;
            return Err(Error::RecoveryRequired("WAL_CHANGED"));
        }
        let required = (MAX_PAYLOAD + HEADER + MARKER_ALLOWANCE) as u64;
        if fs::metadata(self.dir.join("correction.reserve"))?.len() != required {
            self.poisoned = true;
            return Err(Error::RecoveryRequired("RESERVE_CHANGED"));
        }
        crash_at(crash, CrashPoint::BeforeAppend);
        self.poisoned = true; // all failures from here require reopening/recovery
        if correction {
            fs::remove_file(self.dir.join("correction.reserve"))?;
            sync_dir(&self.dir)?;
        }
        self.wal.seek(SeekFrom::Start(self.commit.end_offset))?;
        self.wal.write_all(&encoded)?;
        self.wal.sync_all()?;
        crash_at(crash, CrashPoint::AfterWalSync);
        self.write_marker(&next, crash)?;
        self.commit = next.clone();
        crash_at(crash, CrashPoint::AfterCommit);
        // Failure here leaves a committed correction, but no new admission and no ACK.
        if correction {
            self.reserve()?;
        }
        self.poisoned = false;
        Ok(next)
    }
}
fn crash_at(configured: Option<CrashPoint>, here: CrashPoint) {
    if configured == Some(here) {
        std::process::exit(86);
    }
}

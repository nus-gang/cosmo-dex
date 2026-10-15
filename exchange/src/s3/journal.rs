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

/// Contract canonical JSON: Python ensure_ascii=True, sorted keys, no spaces.
/// Text counts Unicode scalar values; supplementary characters use two escapes.
pub fn canonical(value: &Value) -> Result<Vec<u8>> {
    fn check(v: &Value) -> bool {
        match v {
            Value::Null | Value::Bool(_) => true,
            Value::String(_) => true,
            Value::Array(a) => a.iter().all(check),
            Value::Object(o) => o.iter().all(|(k, v)| k.is_ascii() && check(v)),
            Value::Number(_) => false,
        }
    }
    if !check(value) {
        return Err(Error::InvalidRecord("NON_CANONICAL_VALUE"));
    }
    let json = serde_json::to_string(value).map_err(|_| Error::InvalidRecord("JSON"))?;
    let mut ascii = String::with_capacity(json.len());
    for c in json.chars() {
        if c < '\u{7f}' {
            ascii.push(c);
        } else {
            use std::fmt::Write;
            for u in c.encode_utf16(&mut [0; 2]) {
                write!(&mut ascii, "\\u{u:04x}").unwrap();
            }
        }
    }
    Ok(ascii.into_bytes())
}

pub fn frame(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::ResourceLimit);
    }
    let mut out = Vec::with_capacity(HEADER + payload.len());
    out.extend_from_slice(b"S3W1");
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
    if &header[..4] != b"S3W1" || Sha256::digest(&header[..40])[..] != header[40..] {
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

fn valid_hash(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn validate_context(context: &Value) -> Result<()> {
    let object = context
        .as_object()
        .ok_or(Error::InvalidRecord("S3_CONTEXT"))?;
    if object.len() != 7
        || context["service_schema"] != "s3/3"
        || context["chain_id"] != "nus-s3-dev-1"
        || context["market_id"] != "DEVBASE/DEVQUOTE"
        || context["market_config_version"] != "1"
        || ["genesis_hash", "contract_hash", "config_hash"]
            .iter()
            .any(|k| !valid_hash(&context[k]))
    {
        return Err(Error::InvalidRecord("S3_CONTEXT"));
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
fn secure_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        #[cfg(target_os = "macos")]
        options.custom_flags(0x100); // O_NOFOLLOW
        #[cfg(target_os = "linux")]
        options.custom_flags(0x20000); // O_NOFOLLOW
    }
    options
}
fn check_owner_permissions(metadata: &fs::Metadata, root: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != fs::symlink_metadata(root)?.uid() || metadata.mode() & 0o077 != 0 {
            return Err(Error::RecoveryRequired("EVIDENCE_PERMISSIONS"));
        }
    }
    Ok(())
}
fn check_directory(path: &Path, root: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|_| Error::RecoveryRequired("EVIDENCE_MISSING"))?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(Error::RecoveryRequired("EVIDENCE_DIRECTORY"));
    }
    check_owner_permissions(&m, root)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub command_seq: u64,
    pub record_hash: String,
    pub end_offset: u64,
}
impl Commit {
    pub(crate) fn zero() -> Self {
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
    DuringWalWrite,
}

struct WriterLock(File);
impl WriterLock {
    fn acquire(file: File) -> Result<Self> {
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => Error::WriterAlreadyRunning,
            std::fs::TryLockError::Error(error) => Error::Io(error),
        })?;
        Ok(Self(file))
    }
}
impl Drop for WriterLock {
    fn drop(&mut self) {
        // flock belongs to the open file description. An unrelated fork can
        // retain it until exec even with CLOEXEC, so closing our File alone is
        // insufficient. Only the owning guard unlocks; failed contenders never
        // construct one. This also covers errors before Journal is constructed.
        // On an unlock error, File still closes and a new writer must still pass
        // the OS lock check. Never unlink writer.lock or bypass that check.
        let _ = self.0.unlock();
    }
}

pub struct Journal {
    dir: PathBuf,
    wal: File,
    context: Value,
    commit: Commit,
    poisoned: bool,
    // Fields drop in declaration order: close the WAL before releasing ownership.
    _lock: WriterLock,
}
impl Journal {
    /// Persist raw adapter input before any WAL record refers to it. These bytes
    /// are evidence, not verified consensus facts. The proof verifier is separate.
    pub fn store_evidence(&mut self, bytes: &[u8], media_type: &str) -> Result<Value> {
        if self.poisoned {
            return Err(Error::RecoveryRequired("POISONED_WRITER"));
        }
        let reference =
            super::evidence::reference(bytes, media_type).map_err(Error::InvalidRecord)?;
        super::evidence::verify(&reference, bytes).map_err(Error::InvalidRecord)?;
        let hash = reference["sha256"].as_str().unwrap();
        let root = self.dir.join("objects");
        let dir = root.join("sha256");
        self.poisoned = true;
        for path in [&root, &dir] {
            if fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
                private_dir(path)?;
                sync_dir(path.parent().unwrap())?;
            }
            check_directory(path, &self.dir)?;
        }
        let destination = dir.join(hash);
        if fs::symlink_metadata(&destination).is_ok() {
            self.read_evidence(&reference)?;
            self.poisoned = false;
            return Ok(reference);
        }
        let temporary = dir.join(format!("{hash}.tmp"));
        let mut f = secure_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        // link is atomic NOREPLACE; unlike rename it cannot overwrite a winner.
        // The lock and private namespace prevent cooperating writer races.
        let mut readback = Vec::new();
        secure_options()
            .read(true)
            .open(&temporary)?
            .read_to_end(&mut readback)?;
        super::evidence::verify(&reference, &readback).map_err(Error::InvalidRecord)?;
        // Persist the role as well as the raw digest. Otherwise a restart could
        // adopt identical bytes under a different media type.
        let descriptor = dir.join(format!("{hash}.ref"));
        let descriptor_tmp = dir.join(format!("{hash}.ref.tmp"));
        let mut meta = secure_options()
            .write(true)
            .create_new(true)
            .open(&descriptor_tmp)?;
        meta.write_all(&canonical(&reference)?)?;
        meta.sync_all()?;
        fs::hard_link(&temporary, &destination)?;
        fs::hard_link(&descriptor_tmp, &descriptor)?;
        sync_dir(&dir)?;
        fs::remove_file(&descriptor_tmp)?;
        fs::remove_file(&temporary)?;
        sync_dir(&dir)?;
        self.poisoned = false;
        Ok(reference)
    }
    pub fn read_evidence(&self, reference: &Value) -> Result<Vec<u8>> {
        super::schema::validate("EvidenceRef", reference).map_err(Error::InvalidRecord)?;
        let size = decimal(reference, "byte_length")?;
        let limit = super::evidence::limit(reference["media_type"].as_str().unwrap())
            .map_err(Error::InvalidRecord)?;
        if size == 0 || size > limit as u64 {
            return Err(Error::RecoveryRequired("EVIDENCE_SIZE"));
        }
        let root = self.dir.join("objects");
        let dir = root.join("sha256");
        check_directory(&root, &self.dir)?;
        check_directory(&dir, &self.dir)?;
        let hash = reference["sha256"].as_str().unwrap();
        let descriptor = secure_options()
            .read(true)
            .open(dir.join(format!("{hash}.ref")))
            .map_err(|_| Error::RecoveryRequired("EVIDENCE_DESCRIPTOR_MISSING"))?;
        if !descriptor.metadata()?.is_file() {
            return Err(Error::RecoveryRequired("EVIDENCE_DESCRIPTOR"));
        }
        check_owner_permissions(&descriptor.metadata()?, &self.dir)?;
        let mut meta = Vec::new();
        descriptor.take(4097).read_to_end(&mut meta)?;
        if meta.len() > 4096 || meta != canonical(reference)? {
            return Err(Error::RecoveryRequired("EVIDENCE_DESCRIPTOR"));
        }
        let path = dir.join(hash);
        let f = secure_options()
            .read(true)
            .open(path)
            .map_err(|_| Error::RecoveryRequired("EVIDENCE_MISSING"))?;
        let metadata = f.metadata()?;
        if !metadata.is_file() || metadata.len() != size {
            return Err(Error::RecoveryRequired("EVIDENCE_LENGTH"));
        }
        check_owner_permissions(&metadata, &self.dir)?;
        let mut bytes = Vec::new();
        f.take(size + 1).read_to_end(&mut bytes)?;
        super::evidence::verify(reference, &bytes).map_err(Error::RecoveryRequired)?;
        Ok(bytes)
    }
    fn verify_evidence(&self, reference: &Value) -> Result<()> {
        self.read_evidence(reference).map(|_| ())
    }
    pub fn load_objects(&self, refs: &[Value]) -> Result<super::evidence::Objects> {
        let mut objects = super::evidence::Objects::default();
        for r in refs {
            objects
                .insert(&self.read_evidence(r)?, r["media_type"].as_str().unwrap())
                .map_err(Error::RecoveryRequired)?;
        }
        Ok(objects)
    }
    fn verify_evidence_refs(&self, record: &Value) -> Result<()> {
        let refs = record["evidence_refs"]
            .as_array()
            .ok_or(Error::InvalidRecord("EVIDENCE_REFS"))?;
        if refs
            .windows(2)
            .any(|p| p[0]["sha256"].as_str() >= p[1]["sha256"].as_str())
        {
            return Err(Error::InvalidRecord("EVIDENCE_REF_ORDER"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for reference in refs {
            if !seen.insert(reference["sha256"].as_str().unwrap_or("")) {
                return Err(Error::InvalidRecord("DUPLICATE_EVIDENCE"));
            }
            self.verify_evidence(reference)?;
        }
        Ok(())
    }
    /// Explicit new namespace only. An existing or incomplete directory is never reused.
    pub fn create(path: &Path, context: Value) -> Result<Self> {
        validate_context(&context)?;
        canonical(&context)?;
        private_dir(path)?;
        sync_dir(path.parent().ok_or(Error::InvalidRecord("PARENT"))?)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("writer.lock"))?;
        let lock = WriterLock::acquire(lock)?;
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
        let mut revisions = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join("status.revision"))?;
        revisions.write_all(&frame(&canonical(
            &json!({"context": this.context, "reserved_through":"0"}),
        )?)?)?;
        revisions.sync_all()?;
        this.reserve()?;
        this.write_marker(&Commit::zero(), None)?;
        Ok(this)
    }

    /// Returns committed records only; the caller must replay/verify semantic hashes
    /// before publishing any recovered state. No external effects are sent here.
    pub fn open(path: &Path, context: Value) -> Result<(Self, Vec<Value>)> {
        validate_context(&context)?;
        if fs::symlink_metadata(path.join("profile.guard.json")).is_ok() {
            return Err(Error::RecoveryRequired("DEVELOPMENT_HOME"));
        }
        check_directory(path, path)?;
        // Reject another generation without creating evidence files in its home.
        // Recheck under the writer lock in recover() to catch concurrent changes.
        if fs::read(path.join("context.json"))? != canonical(&context)? {
            return Err(Error::RecoveryRequired("CONTEXT_MISMATCH"));
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("writer.lock"))?;
        let lock = WriterLock::acquire(lock)?;
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
    /// Reserve response revisions under the journal's OS writer lock. Unused
    /// numbers are skipped on restart. This is not an engine command sequence.
    /// Old journals without this sidecar require explicit offline migration;
    /// absence must never silently reset the externally visible counter.
    pub fn reserve_status_revisions(
        &mut self,
        count: u64,
    ) -> Result<std::ops::RangeInclusive<u64>> {
        if self.poisoned {
            return Err(Error::RecoveryRequired("POISONED_SESSION"));
        }
        if count == 0 {
            return Err(Error::InvalidRecord("EMPTY_REVISION_RANGE"));
        }
        self.poisoned = true;
        let temporary = self.dir.join("status.revision.tmp");
        if temporary.exists() {
            return Err(Error::RecoveryRequired("UNCERTAIN_STATUS_REVISION"));
        }
        let destination = self.dir.join("status.revision");
        let mut file = File::open(&destination)?;
        let (value, _, _) =
            read_frame(&mut file)?.ok_or(Error::RecoveryRequired("EMPTY_STATUS_REVISION"))?;
        let previous = decimal(&value, "reserved_through")?;
        if read_frame(&mut file)?.is_some()
            || value != json!({"context":self.context,"reserved_through":previous.to_string()})
        {
            return Err(Error::RecoveryRequired("STATUS_REVISION_BINDING"));
        }
        let end = previous.checked_add(count).ok_or(Error::ResourceLimit)?;
        let mut next = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        next.write_all(&frame(&canonical(&json!({
            "context":self.context,"reserved_through":end.to_string()
        }))?)?)?;
        next.sync_all()?;
        fs::rename(temporary, destination)?;
        sync_dir(&self.dir)?;
        self.poisoned = false;
        Ok((previous + 1)..=end)
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
            self.verify_evidence_refs(&record)?;
            records.push(record);
        }
        if self.commit != expected {
            return Err(Error::RecoveryRequired("COMMITTED_FRAME_MISSING"));
        }
        Ok(records)
    }
    pub(crate) fn preserve_evidence(&self) -> Result<()> {
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
            "bootstrap.json",
            "status.revision",
            "status.revision.tmp",
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
    // Legacy storage-fault fixture only. This reserve does NOT satisfy rc3 B:
    // unlinking it makes blocks available to competing writers. NUS-66 must
    // supply the supported dedicated allocator before any durable ACK service.
    // Kept to preserve the original S3 storage experiments, never for admission.
    fn reserve(&mut self) -> Result<()> {
        #[cfg(feature = "fault-injection")]
        inject_io("reserve")?;
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

    /// Low-level experimental append only. The rc3 capacity certificate and
    /// dedicated allocation are not integrated here. Returning Commit proves
    /// the marker sequence, NOT permission to return a durable service ACK.
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
        #[cfg(feature = "fault-injection")]
        let crash = crash.or_else(|| injected_crash(record));
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
        self.verify_evidence_refs(record)?;
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
        #[cfg(feature = "fault-injection")]
        inject_io("append")?;
        if crash == Some(CrashPoint::DuringWalWrite) {
            self.wal.write_all(&encoded[..encoded.len() / 2])?;
            self.wal.sync_all()?;
            crash_at(crash, CrashPoint::DuringWalWrite);
        }
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

#[cfg(feature = "fault-injection")]
fn injected_crash(record: &Value) -> Option<CrashPoint> {
    if std::env::var("S3_FAULT_SEQ").ok().as_deref() != record["command_seq"].as_str() {
        return None;
    }
    match std::env::var("S3_FAULT_POINT").ok()?.as_str() {
        "before" => Some(CrashPoint::BeforeAppend),
        "partial" => Some(CrashPoint::DuringWalWrite),
        "wal" => Some(CrashPoint::AfterWalSync),
        "marker" => Some(CrashPoint::AfterMarkerSync),
        "rename" => Some(CrashPoint::AfterRename),
        "commit" => Some(CrashPoint::AfterCommit),
        _ => None,
    }
}
#[cfg(feature = "fault-injection")]
fn inject_io(point: &str) -> Result<()> {
    // File trigger lets the harness arm ENOSPC only after initial reserve/ACKs.
    // The test binary remains private to its parent, with no public fault API.
    if std::env::var("S3_FAULT_IO").ok().as_deref() == Some(point)
        && std::env::var_os("S3_FAULT_TRIGGER").is_some_and(|p| Path::new(&p).is_file())
    {
        return Err(Error::Io(std::io::Error::from_raw_os_error(28)));
    }
    Ok(())
}

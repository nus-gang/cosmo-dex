//! Shared bounded public-input transport. Caller supplies a finite reader.
use nus_exchange_contract::s3::{dev_local::Result, journal::sha256};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
/// Bounded transport with exact EOF/hash binding, before home or signer IO.
pub fn read_capture(reader: impl Read, expected: &str) -> Result<Vec<u8>> {
    const MAX: u64 = 48 * 1024 * 1024;
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("CAPTURE_HASH".into());
    }
    let mut raw = Vec::new();
    reader.take(MAX + 1).read_to_end(&mut raw)?;
    if raw.is_empty() || raw.len() as u64 > MAX || sha256(&raw) != expected {
        return Err("CAPTURE_MISMATCH".into());
    }
    Ok(raw)
}

/// Public input files only. Never blocks on FIFO, follows a final link, or
/// accepts a mutable size/identity during the read. No file content in errors.
pub fn read_regular(path: &Path, max: usize) -> Result<Vec<u8>> {
    if std::fs::canonicalize(path)? != path {
        return Err("INPUT_PATH".into());
    }
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let m = f.metadata()?;
    if !m.is_file() || m.nlink() != 1 || m.len() > max as u64 {
        return Err("INPUT_FILE".into());
    }
    let mut raw = Vec::new();
    (&mut f).take(max as u64 + 1).read_to_end(&mut raw)?;
    let n = f.metadata()?;
    let identity = |m: &std::fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.nlink(),
            m.mode(),
        )
    };
    if raw.len() > max || raw.len() as u64 != m.len() || identity(&m) != identity(&n) {
        return Err("INPUT_CHANGED".into());
    }
    Ok(raw)
}

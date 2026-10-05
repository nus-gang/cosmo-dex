//! Descriptor-relative, no-follow IO. No caller-controlled relative paths.
use super::{Error, Result};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    fs::{self, File, Metadata},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};
unsafe extern "C" {
    fn openat(dir: c_int, name: *const c_char, flags: c_int, ...) -> c_int;
    fn mkdirat(dir: c_int, name: *const c_char, mode: u32) -> c_int;
    fn linkat(a: c_int, from: *const c_char, b: c_int, to: *const c_char, flags: c_int) -> c_int;
    fn renameat(a: c_int, from: *const c_char, b: c_int, to: *const c_char) -> c_int;
    fn unlinkat(dir: c_int, name: *const c_char, flags: c_int) -> c_int;
    fn geteuid() -> u32;
    fn dup(fd: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn fdopendir(fd: c_int) -> *mut c_void;
    fn closedir(dir: *mut c_void) -> c_int;
    #[cfg_attr(
        all(target_os = "macos", target_arch = "x86_64"),
        link_name = "readdir$INODE64"
    )]
    fn readdir(dir: *mut c_void) -> *const Dirent;
    #[cfg_attr(target_os = "macos", link_name = "__error")]
    #[cfg_attr(target_os = "linux", link_name = "__errno_location")]
    fn errno_location() -> *mut c_int;

}
#[repr(C)]
struct Dirent {
    ino: u64,
    #[cfg(target_os = "macos")]
    offset: u64,
    #[cfg(target_os = "linux")]
    offset: i64,
    reclen: u16,
    #[cfg(target_os = "macos")]
    namelen: u16,
    kind: u8,
    #[cfg(target_os = "macos")]
    name: [c_char; 1024],
    #[cfg(target_os = "linux")]
    name: [c_char; 256],
}
#[cfg(target_os = "macos")]
const FLAGS: (c_int, c_int, c_int, c_int, c_int) = (0x100, 0x100000, 0x1000000, 0x200, 0x800); // NOFOLLOW DIRECTORY CLOEXEC CREAT EXCL
#[cfg(target_os = "linux")]
const FLAGS: (c_int, c_int, c_int, c_int, c_int) = (0x20000, 0x10000, 0x80000, 0x40, 0x80);
fn name(s: &str) -> Result<CString> {
    if s.is_empty() || s == "." || s == ".." || s.contains('/') {
        return Err(Error::Recovery("PATH_COMPONENT"));
    }
    CString::new(s).map_err(|_| Error::Recovery("PATH_COMPONENT"))
}
fn syscall(n: c_int) -> Result<()> {
    if n < 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}
fn safe(m: &Metadata, dir: bool) -> Result<()> {
    if m.uid() != unsafe { geteuid() }
        || m.mode() & 0o7777 != if dir { 0o700 } else { 0o600 }
        || if dir {
            !m.is_dir()
        } else {
            !m.is_file() || m.nlink() != 1
        }
    {
        return Err(Error::Recovery("FILE_IDENTITY_PERMISSIONS"));
    }
    Ok(())
}
pub(super) struct Dir {
    pub file: File,
}
impl Dir {
    fn open_file(&self, n: &str, write: bool, create: bool, dir: bool) -> Result<File> {
        let n = name(n)?;
        let flags = FLAGS.0
            | FLAGS.2
            | if dir {
                FLAGS.1
            } else {
                if write { 2 } else { 0 }
            }
            | if create { FLAGS.3 | FLAGS.4 } else { 0 };
        let fd = unsafe { openat(self.file.as_raw_fd(), n.as_ptr(), flags, 0o600u32) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let f = unsafe { File::from_raw_fd(fd) };
        safe(&f.metadata()?, dir)?;
        Ok(f)
    }
    pub fn child(&self, n: &str, create: bool) -> Result<Self> {
        if create {
            syscall(unsafe { mkdirat(self.file.as_raw_fd(), name(n)?.as_ptr(), 0o700) })?;
            self.sync()?;
        }
        Ok(Self {
            file: self.open_file(n, false, false, true)?,
        })
    }
    pub fn read(&self, n: &str, limit: usize) -> Result<Vec<u8>> {
        let mut f = self.open_file(n, false, false, false)?;
        if f.metadata()?.len() > limit as u64 {
            return Err(Error::Recovery("FILE_SIZE"));
        }
        let mut raw = vec![];
        (&mut f).take(limit as u64 + 1).read_to_end(&mut raw)?;
        if raw.len() > limit {
            return Err(Error::Recovery("FILE_SIZE"));
        }
        // A concurrent new hardlink is rejected before returning any bytes.
        safe(&f.metadata()?, false)?;
        Ok(raw)
    }
    pub fn file(&self, n: &str, create: bool) -> Result<File> {
        self.open_file(n, true, create, false)
    }
    pub fn sync(&self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }
    pub fn remove(&self, n: &str) -> Result<()> {
        syscall(unsafe { unlinkat(self.file.as_raw_fd(), name(n)?.as_ptr(), 0) })
    }
    pub fn replace(&self, from: &str, to: &str) -> Result<()> {
        syscall(unsafe {
            renameat(
                self.file.as_raw_fd(),
                name(from)?.as_ptr(),
                self.file.as_raw_fd(),
                name(to)?.as_ptr(),
            )
        })
    }
    pub fn write_new(&self, n: &str, raw: &[u8]) -> Result<()> {
        let tmp = format!("{n}.tmp");
        let mut f = self.file(&tmp, true)?;
        f.write_all(raw)?;
        super::fault("file_sync")?;
        f.sync_all()?;
        if self.read(&tmp, raw.len())? != raw {
            return Err(Error::Recovery("READBACK"));
        }
        syscall(unsafe {
            linkat(
                self.file.as_raw_fd(),
                name(&tmp)?.as_ptr(),
                self.file.as_raw_fd(),
                name(n)?.as_ptr(),
                0,
            )
        })?;
        self.remove(&tmp)?;
        super::fault("publish_dir_sync")?;
        self.sync()
    }
    pub fn names(&self) -> Result<Vec<String>> {
        // fdopendir owns a duplicate, never a pathname that could be swapped.
        // dup shares the directory offset: seek to start for every independent scan.
        use std::io::{Seek, SeekFrom};
        let mut borrowed = &self.file;
        borrowed.seek(SeekFrom::Start(0))?;
        let fd = unsafe { dup(self.file.as_raw_fd()) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let ptr = unsafe { fdopendir(fd) };
        if ptr.is_null() {
            unsafe { close(fd) };
            return Err(std::io::Error::last_os_error().into());
        }
        struct Scan(*mut c_void);
        impl Drop for Scan {
            fn drop(&mut self) {
                unsafe { closedir(self.0) };
            }
        }
        let scan = Scan(ptr);
        let mut out = vec![];
        loop {
            unsafe {
                *errno_location() = 0;
            }
            let entry = unsafe { readdir(scan.0) };
            if entry.is_null() {
                if unsafe { *errno_location() } != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                break;
            }
            let n = unsafe { CStr::from_ptr(std::ptr::addr_of!((*entry).name).cast()) }
                .to_str()
                .map_err(|_| Error::Recovery("FILE_NAME"))?;
            if n != "." && n != ".." {
                out.push(n.into());
            }
        }
        out.sort();
        Ok(out)
    }
    pub fn check_file(&self, n: &str, f: &File) -> Result<()> {
        let actual = self.open_file(n, false, false, false)?.metadata()?;
        let old = f.metadata()?;
        safe(&old, false)?;
        if (actual.dev(), actual.ino()) != (old.dev(), old.ino()) {
            return Err(Error::Recovery("FILE_REPLACED"));
        }
        Ok(())
    }
}
// Walk every ancestor from / without following a symlink. Permission checks on
// private roots/children are separate: system ancestors need not be owned by us.
fn absolute_directory(path: &Path) -> Result<File> {
    if !path.is_absolute() {
        return Err(Error::Recovery("ABSOLUTE_PATH"));
    }
    let mut dir = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FLAGS.0 | FLAGS.1 | FLAGS.2)
        .open("/")?;
    for component in path.components() {
        let part = match component {
            std::path::Component::RootDir => continue,
            std::path::Component::Normal(p) => {
                p.to_str().ok_or(Error::Recovery("PATH_ENCODING"))?
            }
            _ => return Err(Error::Recovery("CANONICAL_PATH")),
        };
        let n = name(part)?;
        let fd = unsafe { openat(dir.as_raw_fd(), n.as_ptr(), FLAGS.0 | FLAGS.1 | FLAGS.2) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        dir = unsafe { File::from_raw_fd(fd) };
    }
    Ok(dir)
}
pub(super) struct Root {
    pub dir: Dir,
    pub path: PathBuf,
    pub identity: serde_json::Value,
}
impl Root {
    pub fn create(path: &Path) -> Result<Self> {
        let parent = path.parent().ok_or(Error::Recovery("ROOT_PARENT"))?;
        if fs::canonicalize(parent)?.as_os_str() != parent.as_os_str() {
            return Err(Error::Recovery("CANONICAL_PATH"));
        }
        let parent = Dir {
            file: absolute_directory(parent)?,
        };
        let leaf = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or(Error::Recovery("PATH_ENCODING"))?;
        let created = parent.child(leaf, true)?;
        let root = Self::open(path)?;
        let actual = root.dir.file.metadata()?;
        let expected = created.file.metadata()?;
        if (actual.dev(), actual.ino()) != (expected.dev(), expected.ino()) {
            return Err(Error::Recovery("ROOT_REPLACED"));
        }
        Ok(root)
    }
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute()
            || path.components().any(|c| {
                !matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
            || fs::canonicalize(path)?.as_os_str() != path.as_os_str()
        {
            return Err(Error::Recovery("CANONICAL_PATH"));
        }
        let file = absolute_directory(path)?;
        let m = file.metadata()?;
        safe(&m, true)?;
        let identity = serde_json::json!({"canonical_path":path.to_str().ok_or(Error::Recovery("PATH_ENCODING"))?,"device":m.dev().to_string(),"inode":m.ino().to_string()});
        let root = Self {
            dir: Dir { file },
            path: path.into(),
            identity,
        };
        root.check()?;
        Ok(root)
    }
    pub fn check(&self) -> Result<()> {
        if fs::canonicalize(&self.path)?.as_os_str() != self.path.as_os_str() {
            return Err(Error::Recovery("CANONICAL_PATH"));
        }
        let a = fs::symlink_metadata(&self.path)?;
        let b = self.dir.file.metadata()?;
        safe(&a, true)?;
        safe(&b, true)?;
        if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
            return Err(Error::Recovery("ROOT_REPLACED"));
        }
        Ok(())
    }
}

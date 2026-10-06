//! Private local operator adapter. No REST, key generation, broadcast or logging.
use fips204::{ml_dsa_65, traits::{KeyGen, SerDes, Signer}};
use nus_exchange_contract::s3::settlement_local::chain::OperatorSigner;
use std::{fs::{File, OpenOptions}, io::Read, os::{fd::{AsRawFd, FromRawFd}, unix::{fs::{MetadataExt, OpenOptionsExt}, ffi::OsStrExt}}, path::Path};
use zeroize::Zeroizing;

pub struct LocalSigner {
    public: Vec<u8>,
    secret: ml_dsa_65::PrivateKey,
}
impl LocalSigner {
    /// Read exactly operator.seed beneath an existing private canonical key directory.
    /// Caller obtains expected_public from the pinned genesis/operator configuration.
    pub fn load(directory: &Path, expected_public: &[u8]) -> Result<Self, &'static str> {
        if !directory.is_absolute() || directory.as_os_str().as_bytes().contains(&0)
            || std::fs::canonicalize(directory).map_err(|_| "SIGNER_ROOT")? != directory
            || expected_public.len() != ml_dsa_65::PK_LEN {
            return Err("SIGNER_ROOT");
        }
        let root = OpenOptions::new().read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(directory).map_err(|_| "SIGNER_ROOT")?;
        let rm = root.metadata().map_err(|_| "SIGNER_ROOT")?;
        let uid = unsafe { libc::geteuid() };
        if !rm.is_dir() || rm.uid() != uid || rm.mode() & 0o7777 != 0o700 {
            return Err("SIGNER_ROOT");
        }
        // openat binds lookup to the validated directory inode; NONBLOCK rejects
        // a FIFO without waiting for another process. Never follow a final link.
        let fd = unsafe { libc::openat(root.as_raw_fd(), c"operator.seed".as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK) };
        if fd < 0 { return Err("SIGNER_FILE"); }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let before = file.metadata().map_err(|_| "SIGNER_FILE")?;
        if !before.is_file() || before.uid() != uid || before.mode() & 0o7777 != 0o600
            || before.nlink() != 1 || before.len() != 32 {
            return Err("SIGNER_FILE");
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        file.read_exact(&mut *seed).map_err(|_| "SIGNER_FILE")?;
        let mut extra = [0u8; 1];
        if file.read(&mut extra).map_err(|_| "SIGNER_FILE")? != 0 {
            return Err("SIGNER_FILE");
        }
        let after = file.metadata().map_err(|_| "SIGNER_FILE")?;
        if (before.dev(), before.ino(), before.len(), before.mode(), before.nlink(), before.mtime(), before.mtime_nsec(), before.ctime(), before.ctime_nsec())
            != (after.dev(), after.ino(), after.len(), after.mode(), after.nlink(), after.mtime(), after.mtime_nsec(), after.ctime(), after.ctime_nsec()) {
            return Err("SIGNER_FILE");
        }
        let (public, secret) = ml_dsa_65::KG::keygen_from_seed(&seed);
        let public = public.into_bytes().to_vec();
        if public != expected_public { return Err("SIGNER_PUBLIC_KEY"); }
        Ok(Self { public, secret })
    }
}
impl OperatorSigner for LocalSigner {
    fn public_key(&self) -> &[u8] { &self.public }
    fn sign(&self, document: &[u8]) -> nus_exchange_contract::s3::dev_local::Result<Vec<u8>> {
        if document.is_empty() || document.len() > 139264 { return Err("SIGNER_DOCUMENT".into()); }
        // FIPS 204 deterministic signing, empty context as required by B/L-D.
        // No dependency on unconfigured RNG features and no secret-derived logs.
        self.secret.try_sign_with_seed(&[0u8; 32], document, &[])
            .map(|s| s.to_vec()).map_err(|_| "SIGNER_FAILED".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fips204::traits::Verifier;
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Fixture(std::path::PathBuf, Vec<u8>);
    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::fs::canonicalize(std::env::var("PAPERCLIP_RUN_SCRATCH_DIR").unwrap()).unwrap()
                .join(format!("signer-{}-{name}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(root.join("operator.seed"), [23u8;32]).unwrap();
            std::fs::set_permissions(root.join("operator.seed"), std::fs::Permissions::from_mode(0o600)).unwrap();
            Self(root, ml_dsa_65::KG::keygen_from_seed(&[23;32]).0.into_bytes().to_vec())
        }
        fn load(&self) -> Result<LocalSigner, &'static str> { LocalSigner::load(&self.0, &self.1) }
    }
    impl Drop for Fixture { fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); } }
    #[test] fn valid_signer_and_document_bounds() {
        let f = Fixture::new("valid"); let s = f.load().unwrap();
        assert_eq!(s.public_key(), f.1);
        let sig = s.sign(b"test Cosmos SignDoc").unwrap();
        let pk = ml_dsa_65::PublicKey::try_from_bytes(f.1.clone().try_into().unwrap()).unwrap();
        assert!(pk.verify(b"test Cosmos SignDoc", &sig.try_into().unwrap(), &[]));
        assert!(s.sign(&[]).is_err()); assert!(s.sign(&vec![0;139265]).is_err());
    }
    #[test] fn wrong_public_key() { let mut f=Fixture::new("pk"); f.1[0]^=1; assert!(f.load().is_err()); }
    #[test] fn wrong_root_permissions() { let f=Fixture::new("root"); std::fs::set_permissions(&f.0,std::fs::Permissions::from_mode(0o755)).unwrap(); assert!(f.load().is_err()); }
    #[test] fn wrong_file_permissions_and_lengths() {
        let f=Fixture::new("file"); let p=f.0.join("operator.seed");
        std::fs::set_permissions(&p,std::fs::Permissions::from_mode(0o644)).unwrap(); assert!(f.load().is_err());
        std::fs::set_permissions(&p,std::fs::Permissions::from_mode(0o600)).unwrap();
        for n in [0,31,33] { std::fs::write(&p,vec![0;n]).unwrap(); assert!(f.load().is_err()); }
    }
    #[test] fn hardlink_rejected() { let f=Fixture::new("hard"); std::fs::hard_link(f.0.join("operator.seed"), f.0.join("copy")).unwrap(); assert!(f.load().is_err()); }
    #[test] fn symlink_rejected() {
        let f=Fixture::new("link"); std::fs::rename(f.0.join("operator.seed"), f.0.join("actual")).unwrap();
        symlink("actual",f.0.join("operator.seed")).unwrap(); assert!(f.load().is_err());
        symlink(&f.0,f.0.join("alias")).unwrap(); assert!(LocalSigner::load(&f.0.join("alias"),&f.1).is_err());
    }
    #[test] fn fifo_and_directory_rejected_without_blocking() {
        let f=Fixture::new("fifo"); let p=f.0.join("operator.seed"); std::fs::remove_file(&p).unwrap();
        let c=std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe {libc::mkfifo(c.as_ptr(),0o600)},0); assert!(f.load().is_err());
        std::fs::remove_file(&p).unwrap(); std::fs::create_dir(&p).unwrap(); assert!(f.load().is_err());
    }
}

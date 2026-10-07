//! Private helper argv/byte boundary. Launcher owns descriptor and approval checks.
use sha2::{Digest, Sha256};
use std::{fs::{self, OpenOptions}, io::Read, os::unix::fs::{MetadataExt, OpenOptionsExt}, path::PathBuf};
#[path="direct_child.rs"] mod direct_child;
pub use direct_child::Request;
const REJECT: &str = "DIRECT_HELPER_REJECTED";
const MAX: u64 = 128 * 1024 * 1024;
pub struct Helper { path: PathBuf, hash: String, identity: (u64,u64,u64,u64) }
impl Helper {
    pub fn parse(args: &mut impl Iterator<Item=String>) -> Result<Self, &'static str> {
        if args.next().as_deref()!=Some("--direct-helper") { return Err(REJECT); }
        let path=PathBuf::from(args.next().ok_or(REJECT)?);
        if args.next().as_deref()!=Some("--direct-helper-sha256") { return Err(REJECT); }
        let hash=args.next().ok_or(REJECT)?;
        if !path.is_absolute() || hash.len()!=64 || !hash.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)) { return Err(REJECT); }
        if fs::canonicalize(&path).map_err(|_|REJECT)?!=path { return Err(REJECT); }
        let mut helper=Self{path,hash,identity:(0,0,0,0)};
        helper.identity=helper.inspect()?;
        Ok(helper)
    }
    fn inspect(&self)->Result<(u64,u64,u64,u64), &'static str> {
        let parent=self.path.parent().ok_or(REJECT)?;
        let root=fs::symlink_metadata(parent).map_err(|_|REJECT)?;
        let uid=unsafe{libc::geteuid()};
        if !root.is_dir() || root.mode()&0o7777!=0o700 || root.uid()!=uid {return Err(REJECT)}
        let mut file=OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(&self.path).map_err(|_|REJECT)?;
        let m=file.metadata().map_err(|_|REJECT)?;
        if !m.is_file() || m.mode()&0o7777!=0o500 || m.uid()!=uid || m.nlink()!=1 || m.len()==0 || m.len()>MAX {return Err(REJECT)}
        let mut digest=Sha256::new(); let mut count=0u64; let mut buf=[0u8;65536];
        loop {let n=file.read(&mut buf).map_err(|_|REJECT)?;if n==0{break} count+=n as u64;if count>MAX{return Err(REJECT)}digest.update(&buf[..n]);}
        if count!=m.len() || hex::encode(digest.finalize())!=self.hash {return Err(REJECT)}
        let current=fs::symlink_metadata(&self.path).map_err(|_|REJECT)?;
        let now=fs::symlink_metadata(parent).map_err(|_|REJECT)?;
        if current.dev()!=m.dev() || current.ino()!=m.ino() || now.dev()!=root.dev() || now.ino()!=root.ino() {return Err(REJECT)}
        Ok((root.dev(),root.ino(),m.dev(),m.ino()))
    }
    /// Revalidate the private executable before and after the bounded child.
    /// Success binds only the supplied request, not its authentication/freshness.
    pub fn verify(&self, request: &Request<'_>, stop: &std::sync::atomic::AtomicBool)
        -> Result<(), &'static str> {
        use std::sync::atomic::Ordering;
        if stop.load(Ordering::Relaxed) { return Err(REJECT); }
        self.recheck()?;
        direct_child::verify(&self.path, request, std::time::Duration::from_secs(3), stop)
            .map_err(|_| REJECT)?;
        self.recheck()?;
        if stop.load(Ordering::Relaxed) { return Err(REJECT); }
        Ok(())
    }
    pub fn recheck(&self)->Result<(), &'static str> {
        if self.inspect()?!=self.identity {return Err(REJECT)} Ok(())
    }
}
#[cfg(test)] mod tests {
    use super::*; use std::os::unix::fs::{PermissionsExt,symlink};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new()->Self { let p=PathBuf::from(std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR").unwrap()).canonicalize().unwrap().join(format!("helper-{}-{:?}",std::process::id(),std::thread::current().id()));fs::create_dir(&p).unwrap();fs::set_permissions(&p,fs::Permissions::from_mode(0o700)).unwrap();fs::write(p.join("helper"),b"synthetic").unwrap();fs::set_permissions(p.join("helper"),fs::Permissions::from_mode(0o500)).unwrap();Self(p) }
        fn args(&self)->Vec<String>{vec!["--direct-helper".into(),self.0.join("helper").to_str().unwrap().into(),"--direct-helper-sha256".into(),hex::encode(Sha256::digest(b"synthetic"))]}
    }
    impl Drop for Fixture{fn drop(&mut self){fs::remove_dir_all(&self.0).unwrap();}}
    #[test] fn exact_args_and_replacement(){let f=Fixture::new();let mut a=f.args();a.push("remaining".into());let mut a=a.into_iter();let h=Helper::parse(&mut a).unwrap();assert_eq!(a.next().unwrap(),"remaining");h.recheck().unwrap();fs::rename(f.0.join("helper"),f.0.join("old")).unwrap();fs::copy(f.0.join("old"),f.0.join("helper")).unwrap();assert!(h.recheck().is_err());}
    #[test] fn malformed_and_changed_bytes(){let f=Fixture::new();let a=f.args();for i in 0..4 {let mut bad=a.clone();bad[i]="invalid".into();assert!(Helper::parse(&mut bad.into_iter()).is_err());}let h=Helper::parse(&mut a.into_iter()).unwrap();fs::set_permissions(f.0.join("helper"),fs::Permissions::from_mode(0o700)).unwrap();fs::write(f.0.join("helper"),b"modified!").unwrap();fs::set_permissions(f.0.join("helper"),fs::Permissions::from_mode(0o500)).unwrap();assert!(h.recheck().is_err());}
    #[test] fn links_permissions_and_fifo(){let f=Fixture::new();let p=f.0.join("helper");let a=f.args();fs::set_permissions(&p,fs::Permissions::from_mode(0o700)).unwrap();assert!(Helper::parse(&mut a.clone().into_iter()).is_err());fs::set_permissions(&p,fs::Permissions::from_mode(0o500)).unwrap();fs::hard_link(&p,f.0.join("hard")).unwrap();assert!(Helper::parse(&mut a.clone().into_iter()).is_err());fs::remove_file(f.0.join("hard")).unwrap();fs::rename(&p,f.0.join("old")).unwrap();symlink(f.0.join("old"),&p).unwrap();assert!(Helper::parse(&mut a.clone().into_iter()).is_err());fs::remove_file(&p).unwrap();let c=std::ffi::CString::new(p.to_str().unwrap()).unwrap();assert_eq!(unsafe{libc::mkfifo(c.as_ptr(),0o500)},0);assert!(Helper::parse(&mut a.into_iter()).is_err());}
}

#[cfg(test)] mod invocation_tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};
    struct Fixture(PathBuf);
    impl Drop for Fixture { fn drop(&mut self) {fs::remove_dir_all(&self.0).unwrap();} }
    fn fixture(mutate: bool) -> (Fixture, Helper) {
        let root=PathBuf::from(std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR").unwrap())
            .canonicalize().unwrap().join(format!("helper-call-{}-{:?}",std::process::id(),std::thread::current().id()));
        fs::create_dir(&root).unwrap(); fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
        let path=root.join("helper");
        let response=format!("{{\"version\":1,\"tx_sha256\":\"{}\",\"owner_bound\":true,\"broadcast\":false}}",hex::encode(Sha256::digest(b"synthetic")));
        let mutation=if mutate {"/bin/chmod 700 \"$0\"\nprintf changed > \"$0\"\n/bin/chmod 500 \"$0\"\n"} else {""};
        let source=format!("#!/bin/sh\n/bin/cat >/dev/null\n{}printf '%s\\n' '{}'\n",mutation,response);
        fs::write(&path,&source).unwrap(); fs::set_permissions(&path,fs::Permissions::from_mode(0o500)).unwrap();
        let helper=Helper::parse(&mut vec!["--direct-helper".into(),path.to_str().unwrap().into(),"--direct-helper-sha256".into(),hex::encode(Sha256::digest(source.as_bytes()))].into_iter()).unwrap();
        (Fixture(root),helper)
    }
    fn request() -> Request<'static> {Request{raw:b"synthetic",owner:&[1;20],public_key:&[2;1952],genesis:&[3;32],chain_id:"nus-local",account_number:0,sequence:1}}
    #[test] fn private_child_success_cancel_and_input_rejection() {
        let (_f,h)=fixture(false);
        assert_eq!(h.verify(&request(),&AtomicBool::new(false)),Ok(()));
        assert_eq!(h.verify(&request(),&AtomicBool::new(true)),Err(REJECT));
        let mut bad=request(); bad.owner=&[1;19];
        assert_eq!(h.verify(&bad,&AtomicBool::new(false)),Err(REJECT));
    }
    #[test] fn successful_response_cannot_hide_helper_mutation() {
        let (_f,h)=fixture(true);
        assert_eq!(h.verify(&request(),&AtomicBool::new(false)),Err(REJECT));
        assert!(h.recheck().is_err());
    }
}

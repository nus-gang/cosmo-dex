//! Offline Go verifier transport. Caller must supply a descriptor-pinned private
//! executable and authenticated, same-H account inputs. No approval or broadcast.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{io::{Read, Write, ErrorKind}, os::fd::AsRawFd, path::Path,
    process::{Child, Command, Stdio}, sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant}};
const REJECT: &str = "DIRECT_TX_REJECTED";
const MAX_REQUEST: usize = 192 * 1024;
const MAX_OUTPUT: usize = 512;

pub struct Request<'a> {
    pub raw: &'a [u8], pub owner: &'a [u8], pub public_key: &'a [u8],
    pub genesis: &'a [u8], pub chain_id: &'a str,
    pub account_number: u64, pub sequence: u64,
}
impl Request<'_> {
    fn encode(&self) -> Result<Vec<u8>, &'static str> {
        if self.raw.is_empty() || self.raw.len()>139264 || self.owner.len()!=20 ||
           self.public_key.len()!=1952 || self.genesis.len()!=32 || self.chain_id.is_empty() ||
           self.chain_id.len()>1024 { return Err(REJECT); }
        // Go encoding/json escapes HTML and the two JS line separators.
        let chain=serde_json::to_string(self.chain_id).map_err(|_|REJECT)?
            .replace('<',"\\u003c").replace('>',"\\u003e").replace('&',"\\u0026")
            .replace('\u{2028}',"\\u2028").replace('\u{2029}',"\\u2029");
        let bytes=format!("{{\"raw\":\"{}\",\"owner\":\"{}\",\"public_key\":\"{}\",\"genesis\":\"{}\",\"chain_id\":{},\"account_number\":\"{}\",\"sequence\":\"{}\"}}",
            STANDARD.encode(self.raw),STANDARD.encode(self.owner),STANDARD.encode(self.public_key),
            STANDARD.encode(self.genesis),chain,self.account_number,self.sequence).into_bytes();
        if bytes.len()>MAX_REQUEST { return Err(REJECT); } Ok(bytes)
    }
    fn response(&self) -> Vec<u8> {
        format!("{{\"version\":1,\"tx_sha256\":\"{}\",\"owner_bound\":true,\"broadcast\":false}}\n",Sha256::digest(self.raw).iter().map(|b|format!("{b:02x}")).collect::<String>()).into_bytes()
    }
}
struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); }
}
fn nonblock(pipe: &impl AsRawFd) -> Result<(), &'static str> {
    unsafe {
        let flags=libc::fcntl(pipe.as_raw_fd(),libc::F_GETFL);
        if flags<0 || libc::fcntl(pipe.as_raw_fd(),libc::F_SETFL,flags|libc::O_NONBLOCK)<0 { return Err(REJECT); }
    } Ok(())
}
// One bounded read per loop prevents a continuously-writing child starving the deadline.
fn read_once(pipe: &mut impl Read, bytes: &mut Vec<u8>) -> Result<bool, &'static str> {
    let mut buf=[0u8;MAX_OUTPUT+1];
    match pipe.read(&mut buf) {
        Ok(0)=>Ok(true),
        Ok(n)=>{ if bytes.len()+n>MAX_OUTPUT {return Err(REJECT)} bytes.extend_from_slice(&buf[..n]); Ok(false) },
        Err(e) if matches!(e.kind(),ErrorKind::WouldBlock|ErrorKind::Interrupted)=>Ok(false),
        Err(_)=>Err(REJECT),
    }
}
/// No retries, no inherited credentials, no shell; exact stdout, empty stderr,
/// successful exit and both EOFs required. All exit/error/unwind paths reap.
/// Timeout covers pipe IO and execution after spawn, not OS spawn scheduling.
pub fn verify(executable: &Path, request: &Request<'_>, timeout: Duration, stop: &AtomicBool) -> Result<(), &'static str> {
    let input=request.encode()?;
    if !executable.is_absolute() || timeout.is_zero() || timeout>Duration::from_secs(3) || stop.load(Ordering::Relaxed) { return Err(REJECT); }
    let start=Instant::now();
    let mut child=Reap(Command::new(executable).args(["verify","--local-demo-profile","nus-s3-local-demo","--acknowledge-unproven-space"])
        .env_clear().current_dir("/").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|_|REJECT)?);
    let mut stdin=child.0.stdin.take();
    let mut stdout=child.0.stdout.take().ok_or(REJECT)?;
    let mut stderr=child.0.stderr.take().ok_or(REJECT)?;
    nonblock(stdin.as_ref().ok_or(REJECT)?)?; nonblock(&stdout)?; nonblock(&stderr)?;
    let(mut offset,mut out,mut err,mut out_eof,mut err_eof)=(0,Vec::new(),Vec::new(),false,false);
    loop {
        if stop.load(Ordering::Relaxed) || start.elapsed()>=timeout { return Err(REJECT); }
        if let Some(pipe)=stdin.as_mut() {
            match pipe.write(&input[offset..]) {
                Ok(0)=>return Err(REJECT), Ok(n)=>offset+=n,
                Err(e) if matches!(e.kind(),ErrorKind::WouldBlock|ErrorKind::Interrupted)=>{},
                Err(_)=>return Err(REJECT),
            }
            if offset==input.len() { stdin.take(); }
        }
        if !out_eof { out_eof=read_once(&mut stdout,&mut out)?; }
        if !err_eof { err_eof=read_once(&mut stderr,&mut err)?; }
        if !err.is_empty() {return Err(REJECT)}
        if let Some(status)=child.0.try_wait().map_err(|_|REJECT)? {
            if !status.success() {return Err(REJECT)}
            if out_eof && err_eof {
                if offset!=input.len() || out!=request.response() {return Err(REJECT)}
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs,os::unix::fs::PermissionsExt,sync::atomic::AtomicU64};
    static NEXT:AtomicU64=AtomicU64::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(body:&str)->Self {
            let root=std::env::var_os("PAPERCLIP_RUN_SCRATCH_DIR").expect("run scratch");
            let dir=std::path::PathBuf::from(root).join(format!("direct-child-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
            fs::create_dir(&dir).unwrap(); fs::set_permissions(&dir,fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(dir.join("helper"),format!("#!/bin/sh\nprintf '%s' $$ > '{}'\n{}\n",dir.join("pid").display(),body.replace("INPUT_PLACEHOLDER",dir.join("input").to_str().unwrap()))).unwrap();
            fs::set_permissions(dir.join("helper"),fs::Permissions::from_mode(0o500)).unwrap();Self(dir)
        }
        fn run(&self,r:&Request<'_>,timeout:Duration,stop:&AtomicBool)->Result<(), &'static str>{verify(&self.0.join("helper"),r,timeout,stop)}
        fn reaped(&self){ if let Ok(raw)=fs::read_to_string(self.0.join("pid")) { let pid:i32=raw.parse().unwrap(); assert_eq!(unsafe{libc::kill(pid,0)},-1);assert_eq!(std::io::Error::last_os_error().raw_os_error(),Some(libc::ESRCH)); } else {panic!("child never started")} }
    }
    impl Drop for Fixture {fn drop(&mut self){fs::remove_dir_all(&self.0).unwrap();}}
    fn request()->Request<'static>{Request{raw:b"synthetic",owner:&[1;20],public_key:&[2;1952],genesis:&[3;32],chain_id:"nus-local",account_number:0,sequence:u64::MAX}}
    #[test]
    fn exact_finite_transport_and_environment(){
        let r=request();let response=String::from_utf8(r.response()).unwrap();
        let f=Fixture::new(&format!("[ \"$#\" = 4 ] || exit 2\n[ \"$1\" = verify ] || exit 2\n[ -z \"$PAPERCLIP_API_KEY\" ] || exit 2\n/bin/cat > 'INPUT_PLACEHOLDER'\nprintf '%s' '{}'",response));
        assert_eq!(f.run(&r,Duration::from_secs(2),&AtomicBool::new(false)),Ok(())); f.reaped();
        assert_eq!(fs::read(f.0.join("input")).unwrap(),r.encode().unwrap());
    }
    #[test]
    fn reject_bad_output_exit_and_output_flood(){
        for body in ["/bin/cat >/dev/null; printf wrong", "printf secret >&2; exit 2", "while :; do printf '0123456789'; done", "exit 2"] {
            let f=Fixture::new(body);assert_eq!(f.run(&request(),Duration::from_secs(1),&AtomicBool::new(false)),Err(REJECT));f.reaped();
        }
    }
    #[test]
    fn blocked_write_timeout_and_cancellation_reap(){
        let bytes=vec![4;139264];let mut r=request();r.raw=&bytes;
        let f=Fixture::new("exec /bin/sleep 10"); let now=Instant::now();
        assert_eq!(f.run(&r,Duration::from_millis(600),&AtomicBool::new(false)),Err(REJECT));
        assert!(now.elapsed()<Duration::from_secs(2)); f.reaped();
        let f=Fixture::new("exec /bin/sleep 10");let stop=AtomicBool::new(false);
        std::thread::scope(|s|{s.spawn(||{std::thread::sleep(Duration::from_millis(600));stop.store(true,Ordering::Relaxed)});
            assert_eq!(f.run(&request(),Duration::from_secs(2),&stop),Err(REJECT));});f.reaped();
    }
    #[test]
    fn invalid_inputs_spawn_nothing_and_go_escaping(){
        let f=Fixture::new("exit 0");let mut r=request();r.raw=&[];
        assert_eq!(f.run(&r,Duration::from_secs(1),&AtomicBool::new(false)),Err(REJECT));
        assert_eq!(f.run(&request(),Duration::from_secs(4),&AtomicBool::new(false)),Err(REJECT));
        assert_eq!(f.run(&request(),Duration::from_secs(1),&AtomicBool::new(true)),Err(REJECT));
        assert!(!f.0.join("pid").exists());
        r=request();r.chain_id="<&>\u{2028}\u{2029}";
        assert!(String::from_utf8(r.encode().unwrap()).unwrap().contains("\\u003c\\u0026\\u003e\\u2028\\u2029"));
    }
    #[test]
    #[ignore = "requires explicit Go fixture and compiled offline helper"]
    fn actual_go_helper_signed_input() {
        let binary=std::env::var_os("DIRECT_IPC_BINARY").expect("Go helper");
        let bytes=fs::read(std::env::var_os("DIRECT_IPC_FIXTURE").expect("Go fixture")).unwrap();
        let v:serde_json::Value=serde_json::from_slice(&bytes).unwrap();
        let dec=|key:&str|STANDARD.decode(v[key].as_str().unwrap()).unwrap();
        let(raw,owner,key,genesis)=(dec("raw"),dec("owner"),dec("public_key"),dec("genesis"));
        let mut r=Request{raw:&raw,owner:&owner,public_key:&key,genesis:&genesis,
            chain_id:v["chain_id"].as_str().unwrap(),account_number:v["account_number"].as_str().unwrap().parse().unwrap(),
            sequence:v["sequence"].as_str().unwrap().parse().unwrap()};
        assert_eq!(r.encode().unwrap(),bytes);
        assert_eq!(verify(Path::new(&binary),&r,Duration::from_secs(3),&AtomicBool::new(false)),Ok(()));
        r.sequence+=1;
        assert_eq!(verify(Path::new(&binary),&r,Duration::from_secs(3),&AtomicBool::new(false)),Err(REJECT));
        r.sequence-=1;
        r.owner=&[9;20];
        assert_eq!(verify(Path::new(&binary),&r,Duration::from_secs(3),&AtomicBool::new(false)),Err(REJECT));
    }

}

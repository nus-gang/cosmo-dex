mod support;
use nus_exchange_contract::s2::journal::{canonical, sha256};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use support::*;
struct Process {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Process {
    fn start(d: &Dir, create: bool, fault: Option<(&str, &str)>, io: Option<&str>) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_exchange-s2"));
        cmd.arg(if create { "create" } else { "open" })
            .arg(d.0.join("manifest.json"))
            .arg(d.0.join("journal"));
        if create {
            cmd.arg(d.0.join("bootstrap.json"));
        }
        if let Some((seq, point)) = fault {
            cmd.env("S2_FAULT_SEQ", seq).env("S2_FAULT_POINT", point);
        }
        if let Some(point) = io {
            cmd.env("S2_FAULT_IO", point)
                .env("S2_FAULT_TRIGGER", d.0.join("trigger"));
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }
    fn send(&mut self, v: &Value) {
        self.input.write_all(&canonical(v).unwrap()).unwrap();
        self.input.write_all(b"\n").unwrap();
        self.input.flush().unwrap();
    }
    fn receive(&mut self) -> Option<Value> {
        let mut s = String::new();
        if self.output.read_line(&mut s).unwrap() == 0 {
            None
        } else {
            Some(serde_json::from_str(&s).unwrap())
        }
    }
    fn call(&mut self, v: &Value) -> Value {
        self.send(v);
        self.receive().expect("service response")
    }
    fn login(&mut self, id: &str) -> String {
        let ch = self.call(&challenge(id));
        let response = self.call(&login_messages(id, &ch));
        assert_eq!(response["http_status"], "200", "{response}");
        response["body"]["token"].as_str().unwrap().into()
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn setup() -> (Dir, Value) {
    let d = Dir::new();
    let s = snapshot(now());
    fs::write(d.0.join("manifest.json"), canonical(&manifest(&s)).unwrap()).unwrap();
    fs::write(d.0.join("bootstrap.json"), canonical(&s).unwrap()).unwrap();
    (d, s)
}
fn startup(d: &Dir, s: &Value, fault: Option<(&str, &str)>, io: Option<&str>) -> (Process, String) {
    let mut p = Process::start(d, true, fault, io);
    let token = p.login("order");
    assert_eq!(p.call(&observe(s, now()))["body"]["mode"], "OPEN");
    (p, token)
}
fn evidence(name: &str, value: &Value) {
    if let Ok(path) = std::env::var("S2_PROCESS_EVIDENCE") {
        fs::create_dir_all(&path).unwrap();
        fs::write(
            Path::new(&path).join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}
fn copy_files(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for item in fs::read_dir(from).unwrap() {
        let item = item.unwrap();
        if item.file_type().unwrap().is_file() {
            fs::copy(item.path(), to.join(item.file_name())).unwrap();
        }
    }
}
#[test]
fn process_writer_lock_kill_restart_original_receipt_and_private_session() {
    let (d, s) = setup();
    let (mut p, token) = startup(&d, &s, None, None);
    let mut second = Process::start(&d, false, None, None);
    assert!(second.receive().is_none());
    assert!(!second.child.wait().unwrap().success());
    drop(second);
    let accepted = p.call(&command(&s, "order", &token, &[]));
    assert_eq!(accepted["http_status"], "201");
    let book = p.call(&request("GET", "/s2/book", None, b""));
    let before = fs::read(d.0.join("journal/journal.wal")).unwrap();
    p.child.kill().unwrap();
    p.child.wait().unwrap();
    drop(p);
    let mut p = Process::start(&d, false, None, None);
    assert_eq!(
        p.call(&request("GET", "/s2/me", Some(&token), b""))["http_status"],
        "401"
    );
    assert_eq!(
        p.call(&request("GET", "/s2/status", None, b""))["body"]["mode"],
        "CATCHING_UP"
    );
    assert_eq!(p.call(&request("GET", "/s2/book", None, b"")), book);
    let token = p.login("order");
    let retry = p.call(&command(&s, "order", &token, &[]));
    assert_eq!(retry["body"], accepted["body"]);
    assert_eq!(retry["http_status"], "200");
    assert_eq!(fs::read(d.0.join("journal/journal.wal")).unwrap(), before);
    evidence(
        "kill-restart",
        &json!({"ack":accepted,"retry":retry,"book":book,"wal_sha256":sha256(&before),"external_sends":"0"}),
    );
}
#[test]
fn committed_process_corruption_is_preserved_and_never_resets() {
    let (d, s) = setup();
    let (mut p, token) = startup(&d, &s, None, None);
    let ack = p.call(&command(&s, "order", &token, &[]));
    drop(p);
    let mut cases = Vec::new();
    for kind in [
        "missing",
        "header",
        "payload",
        "partial",
        "marker",
        "bootstrap",
        "status",
    ] {
        let copy = Dir::new();
        fs::copy(d.0.join("manifest.json"), copy.0.join("manifest.json")).unwrap();
        copy_files(&d.0.join("journal"), &copy.0.join("journal"));
        let wal = copy.0.join("journal/journal.wal");
        let mut bytes = fs::read(&wal).unwrap();
        match kind {
            "missing" => bytes.clear(),
            "header" => bytes[4] ^= 1,
            "payload" => bytes[72] ^= 1,
            "partial" => {
                bytes.pop();
            }
            "marker" => fs::write(copy.0.join("journal/commit.marker"), b"bad").unwrap(),
            "bootstrap" => fs::write(copy.0.join("journal/bootstrap.json"), b"bad").unwrap(),
            "status" => fs::write(copy.0.join("journal/status.revision"), b"bad").unwrap(),
            _ => unreachable!(),
        }
        fs::write(&wal, &bytes).unwrap();
        let mut p = Process::start(&copy, false, None, None);
        assert!(p.receive().is_none(), "{kind}");
        assert!(!p.child.wait().unwrap().success());
        assert_eq!(fs::read(&wal).unwrap(), bytes);
        cases.push(json!({"case":kind,"recovery":"REFUSED","original_wal_preserved":true}));
    }
    evidence("corruption", &json!({"ack":ack,"cases":cases}));
}
#[cfg(feature = "fault-injection")]
#[test]
fn process_crashes_at_signed_commit_boundaries_keep_ack_prefix_or_stop() {
    let mut cases = Vec::new();
    for point in ["before", "partial", "wal", "marker", "rename", "commit"] {
        let (d, s) = setup();
        let (mut p, token) = startup(&d, &s, Some(("2", point)), None);
        let ack = p.call(&command(&s, "order", &token, &[]));
        assert_eq!(ack["http_status"], "201");
        let buyer = p.login("buyer-order");
        p.send(&command(&s, "buyer-order", &buyer, &[]));
        assert!(p.receive().is_none());
        assert_eq!(p.child.wait().unwrap().code(), Some(86));
        drop(p);
        let original = fs::read(d.0.join("journal/journal.wal")).unwrap();
        let marker = fs::read(d.0.join("journal/commit.marker")).unwrap();
        let mut recovered = Process::start(&d, false, None, None);
        let result = if matches!(point, "partial" | "wal" | "marker") {
            assert!(recovered.receive().is_none());
            assert!(!recovered.child.wait().unwrap().success());
            "RECOVERY_REQUIRED"
        } else {
            let token = recovered.login("order");
            assert_eq!(
                recovered.call(&command(&s, "order", &token, &[]))["body"],
                ack["body"]
            );
            let status = recovered.call(&request("GET", "/s2/status", None, b""));
            assert_eq!(
                status["body"]["stream_seq"],
                if point == "before" { "1" } else { "2" }
            );
            if point != "before" {
                let token = recovered.login("buyer-order");
                let retry = recovered.call(&command(&s, "buyer-order", &token, &[]));
                assert_eq!(retry["http_status"], "200");
                let view = recovered.call(&request("GET", "/s2/me", Some(&token), b""));
                assert_eq!(view["body"]["fills"].as_array().unwrap().len(), 1);
            }
            "REPLAYED"
        };
        assert_eq!(fs::read(d.0.join("journal/journal.wal")).unwrap(), original);
        assert_eq!(fs::read(d.0.join("journal/commit.marker")).unwrap(), marker);
        cases.push(json!({"point":point,"previous_ack":ack["body"],"outcome":result,"wal_sha256":sha256(&original),"external_sends":"0"}));
    }
    evidence("crash-boundaries", &json!(cases));
}
#[cfg(feature = "fault-injection")]
#[test]
fn correction_reserve_io_failure_returns_unknown_and_replays_commit() {
    for point in ["append", "reserve"] {
        let (d, s) = setup();
        let (mut p, token) = startup(&d, &s, None, Some(point));
        p.call(&command(&s, "order", &token, &[]));
        let buyer = p.login("buyer-order");
        p.call(&command(&s, "buyer-order", &buyer, &[]));
        let prior = p.call(&request("GET", "/s2/me", Some(&token), b""));
        let mut next = s.clone();
        next["body"]["observed_height"] = json!("101");
        for a in next["body"]["accounts"].as_array_mut().unwrap() {
            a["owner_epoch"] = json!("1");
        }
        rehash(&mut next);
        fs::write(d.0.join("trigger"), b"inject ENOSPC").unwrap();
        let result = p.call(&observe(&next, now()));
        assert_eq!(result["body"]["state"], "SUBMISSION_UNKNOWN");
        assert_eq!(result["http_status"], "503");
        assert_eq!(
            p.call(&request("GET", "/s2/me", Some(&token), b""))["body"]["code"],
            "RECOVERY_REQUIRED"
        );
        drop(p);
        fs::remove_file(d.0.join("trigger")).unwrap();
        let mut p = Process::start(&d, false, None, None);
        let token = p.login("order");
        let view = p.call(&request("GET", "/s2/me", Some(&token), b""));
        if point == "reserve" {
            assert_eq!(view["body"]["fills"][0]["state"], "CORRECTED");
            assert_eq!(view["body"]["ledger"][0]["D"], "0");
        } else {
            assert_eq!(view["body"]["ledger"], prior["body"]["ledger"]);
        }
        assert!(
            fs::metadata(d.0.join("journal/correction.reserve"))
                .unwrap()
                .len()
                > 16_777_216
        );
        evidence(
            &format!("injected-enospc-{point}"),
            &json!({"result":result,"recovered":view,"kind":"injected errno 28, not host disk exhaustion"}),
        );
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn correction_process_crashes_never_publish_partial_unwind() {
    let mut cases = Vec::new();
    for point in ["before", "partial", "wal", "marker", "rename", "commit"] {
        let (d, s) = setup();
        let (mut p, token) = startup(&d, &s, Some(("3", point)), None);
        let sell = p.call(&command(&s, "order", &token, &[]));
        let token = p.login("buyer-order");
        let buy = p.call(&command(&s, "buyer-order", &token, &[]));
        let mut next = s.clone();
        next["body"]["observed_height"] = json!("101");
        for a in next["body"]["accounts"].as_array_mut().unwrap() {
            a["owner_epoch"] = json!("1");
        }
        rehash(&mut next);
        p.send(&observe(&next, now()));
        assert!(p.receive().is_none());
        assert_eq!(p.child.wait().unwrap().code(), Some(86));
        drop(p);
        let original = fs::read(d.0.join("journal/journal.wal")).unwrap();
        let mut recovered = Process::start(&d, false, None, None);
        let result = if matches!(point, "partial" | "wal" | "marker") {
            assert!(recovered.receive().is_none());
            assert!(!recovered.child.wait().unwrap().success());
            "RECOVERY_REQUIRED"
        } else {
            for (id, receipt) in [("order", &sell), ("buyer-order", &buy)] {
                let token = recovered.login(id);
                assert_eq!(
                    recovered.call(&command(&s, id, &token, &[]))["body"],
                    receipt["body"]
                );
                let view = recovered.call(&request("GET", "/s2/me", Some(&token), b""));
                assert_eq!(
                    view["body"]["fills"][0]["state"],
                    if point == "before" {
                        "PENDING"
                    } else {
                        "CORRECTED"
                    }
                );
            }
            "REPLAYED"
        };
        assert_eq!(fs::read(d.0.join("journal/journal.wal")).unwrap(), original);
        cases.push(json!({"point":point,"outcome":result,"ack_receipts":[sell["body"],buy["body"]],"wal_sha256":sha256(&original)}));
    }
    evidence("correction-crashes", &json!(cases));
}

#[test]
fn external_ack_ledger_detects_joint_local_file_rollback() {
    let (d, s) = setup();
    let (mut p, token) = startup(&d, &s, None, None);
    p.call(&command(&s, "order", &token, &[]));
    let wal = fs::read(d.0.join("journal/journal.wal")).unwrap();
    let marker = fs::read(d.0.join("journal/commit.marker")).unwrap();
    let buyer = p.login("buyer-order");
    let last_ack = p.call(&command(&s, "buyer-order", &buyer, &[]));
    assert_eq!(last_ack["body"]["command_seq"], "2");
    drop(p);
    // Deliberate rollback in this isolated test namespace. The external ACK is
    // retained outside it. No local-only rollback detection is claimed.
    fs::write(d.0.join("journal/journal.wal"), wal).unwrap();
    fs::write(d.0.join("journal/commit.marker"), marker).unwrap();
    let mut p = Process::start(&d, false, None, None);
    let status = p.call(&request("GET", "/s2/status", None, b""));
    assert_ne!(
        status["body"]["stream_seq"],
        last_ack["body"]["command_seq"]
    );
    evidence(
        "joint-rollback",
        &json!({"external_ack":last_ack,"recovered_status":status,"external_comparison":"ROLLBACK_DETECTED","local_only_detection":false}),
    );
}

//! Step 34 — SSH sessions and saved credentials (ADR 0067). `SUBAK-FR-061`, `KIM-SEC-013`.
//!
//! Real Hosts, Windows' real OpenSSH client and a real SSH server: Microsoft's portable sshd
//! (`scripts/fetch-test-sshd.ps1` puts it in `target/test-sshd`), run as this user on 127.0.0.1
//! with a throwaway host key and key-only login. Scenarios: a refused save for an unknown host,
//! first trust (the user answers ssh's own question), a saved passphrase answering once, the
//! secret never appearing in JSON, config, state or the screen, a wrong saved secret failing
//! without a retry loop, a changed host key refused, the server offline, SSH profiles.
//! Without the server the test says so and does nothing (it never downloads).
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::Value;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NO_WINDOW: u32 = 0x0800_0000;

fn sshd_dir() -> Option<PathBuf> {
    let d = std::env::var_os("SUBAK_TEST_SSHD_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-sshd/OpenSSH-Win64"),
        PathBuf::from,
    );
    d.join("sshd.exe").is_file().then_some(d)
}

fn fwd(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

struct Server {
    exe: PathBuf,
    config: PathBuf,
    child: Option<Child>,
}

impl Server {
    fn start(&mut self) {
        let child = Command::new(&self.exe)
            .args(["-D", "-e", "-f"])
            .arg(&self.config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(NO_WINDOW)
            .spawn()
            .unwrap();
        self.child = Some(child);
        std::thread::sleep(Duration::from_millis(800));
    }

    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Lab {
    ep: String,
    appdata: PathBuf,
    dir: PathBuf,
    started: bool,
    creds: Vec<String>,
}

impl Lab {
    fn cli(&self, args: &[&str]) -> (i32, Value) {
        let local = matches!(args.first(), Some(&("profile" | "config")))
            || args.starts_with(&["ssh", "credential"]);
        let mut a = vec!["--json"];
        if !local {
            a.extend(["--endpoint", self.ep.as_str()]);
        }
        a.extend_from_slice(args);
        let (code, doc) = run_env(&a, &[("APPDATA", self.appdata.to_str().unwrap())]);
        validate("subak-cli-v1", &doc);
        (code, doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    /// `ssh credential save`, the secret on stdin (as a script would give it).
    fn save(&mut self, target: &str, kh: &str, secret: &str) -> (i32, String) {
        let mut c = Command::new(subak())
            .args([
                "--json",
                "ssh",
                "credential",
                "save",
                target,
                "--option",
                kh,
            ])
            .env("APPDATA", &self.appdata)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(NO_WINDOW)
            .spawn()
            .unwrap();
        writeln!(c.stdin.take().unwrap(), "{secret}").unwrap();
        let out = c.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        validate("subak-cli-v1", &doc);
        if let Some(c) = doc["data"]["credential"].as_str() {
            self.creds.push(c.to_string());
        }
        (out.status.code().unwrap_or(-1), text)
    }

    fn screen(&self, t: &str) -> String {
        let (code, d) = self.cli(&["capture", t]);
        if code != 0 {
            return String::new();
        }
        d["data"]["lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait_screen(&self, t: &str, want: &str) -> String {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let s = self.screen(t);
            if s.contains(want) {
                return s;
            }
            assert!(Instant::now() < end, "{t} never showed {want:?}:\n{s}");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn keys(&self, t: &str, text: &str) {
        self.ok(&["command", "--target", t, &format!("send-keys -l '{text}'")]);
        self.ok(&["command", "--target", t, "send-keys Enter"]);
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        if self.started {
            let _ = run(&[
                "host",
                "stop",
                "--endpoint",
                &self.ep,
                "--kill-terminals",
                "--yes",
            ]);
        }
        for c in &self.creds {
            let target = c.trim_start_matches("mouv-subak:ssh:");
            let _ = run_env(
                &["ssh", "credential", "forget", target],
                &[("APPDATA", self.appdata.to_str().unwrap())],
            );
        }
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn keygen(bin: &Path, file: &Path, pass: &str) {
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_file(file.with_extension("pub"));
    let st = Command::new(bin.join("ssh-keygen.exe"))
        .args(["-q", "-t", "ed25519", "-N", pass, "-f"])
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .creation_flags(NO_WINDOW)
        .status()
        .unwrap();
    assert!(st.success());
}

#[test]
fn ssh_sessions_trust_hosts_explicitly_and_keep_secrets_in_credential_manager() {
    let Some(bin) = sshd_dir() else {
        eprintln!(
            "NOT TESTED: no test sshd (run `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-test-sshd.ps1`)"
        );
        return;
    };
    let tag = format!("w34-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("subak-ssh34-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let appdata = dir.join("appdata");
    std::fs::create_dir_all(appdata.join("mouv").join("subak")).unwrap();
    let mut lab = Lab {
        ep: endpoint(&tag),
        appdata,
        dir: dir.clone(),
        started: false,
        creds: Vec::new(),
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let host_key = dir.join("host_ed25519");
    let plain = dir.join("plain_key");
    let locked = dir.join("locked_key");
    let pass = format!("pass {} ✓-phrase", std::process::id());
    keygen(&bin, &host_key, "");
    keygen(&bin, &plain, "");
    keygen(&bin, &locked, &pass);
    let pubs = [&plain, &locked]
        .iter()
        .map(|k| std::fs::read_to_string(k.with_extension("pub")).unwrap())
        .collect::<String>();
    std::fs::write(dir.join("authorized_keys"), pubs).unwrap();
    let cfg = dir.join("sshd_config");
    std::fs::write(
        &cfg,
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nStrictModes no\nPidFile {}\nSubsystem sftp none\n",
            fwd(&host_key),
            fwd(&dir.join("authorized_keys")),
            fwd(&dir.join("sshd.pid"))
        ),
    )
    .unwrap();
    let mut server = Server {
        exe: bin.join("sshd.exe"),
        config: cfg,
        child: None,
    };
    server.start();
    let user = std::env::var("USERNAME").unwrap().to_lowercase();
    let target = format!("{user}@127.0.0.1:{port}");
    let kh_file = dir.join("known_hosts");
    let kh = format!("UserKnownHostsFile={}", fwd(&kh_file));
    let plain_s = plain.display().to_string();
    let locked_s = locked.display().to_string();
    let only = "IdentitiesOnly=yes";

    // An unknown host gets no saved secret.
    let (code, text) = lab.save(&target, &kh, &pass);
    assert_ne!(code, 0, "{text}");
    assert!(
        text.contains("not in known_hosts") && !text.contains(&pass),
        "{text}"
    );

    // First trust: ssh asks, the user answers.
    lab.started = true;
    let d = lab.ok(&[
        "ssh",
        "connect",
        &target,
        "first",
        "--identity",
        &plain_s,
        "--option",
        &kh,
        "--option",
        only,
    ]);
    assert_eq!(d["ssh"]["host_verified"], false);
    assert_eq!(d["ssh"]["saved_secret_used"], false);
    let args: Vec<&str> = d["ssh"]["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        args.windows(2)
            .any(|w| w == ["-o", "StrictHostKeyChecking=ask"]),
        "{args:?}"
    );
    lab.wait_screen("first", "continue connecting");
    lab.keys("first", "yes");
    std::thread::sleep(Duration::from_millis(1500));
    lab.keys("first", "echo marker-%OS%");
    lab.wait_screen("first", "marker-Windows_NT");
    assert!(kh_file.is_file(), "the user's answer wrote known_hosts");
    let st = lab.ok(&["ssh", "credential", "status", &target, "--option", &kh]);
    assert_eq!(
        (st["host_verified"].clone(), st["saved"].clone()),
        (true.into(), false.into())
    );

    // A saved passphrase answers the key's prompt once; nobody types it.
    let (code, text) = lab.save(&target, &kh, &pass);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains(&pass), "the secret is never printed: {text}");
    let d = lab.ok(&[
        "ssh",
        "connect",
        &target,
        "saved",
        "--identity",
        &locked_s,
        "--option",
        &kh,
        "--option",
        only,
    ]);
    assert_eq!(d["ssh"]["saved_secret_used"], true, "{d}");
    std::thread::sleep(Duration::from_millis(2500));
    lab.keys("saved", "echo marker-%OS%");
    let s = lab.wait_screen("saved", "marker-Windows_NT");
    assert!(!s.contains("Enter passphrase"), "{s}");

    // SSH profiles (several) in config.toml; no secret there.
    std::fs::write(
        lab.appdata.join("mouv").join("subak").join("config.toml"),
        format!(
            "config_version = 1\n\n[[profiles]]\nname = \"lab-plain\"\nssh = \"{target}\"\nidentity = '{plain_s}'\n\n[[profiles]]\nname = \"lab-locked\"\nssh = \"{target}\"\nidentity = '{locked_s}'\nenv = {{ LAB = \"1\" }}\n"
        ),
    )
    .unwrap();
    let list = lab.ok(&["profile", "list"]);
    let ssh_profiles = list["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["ssh"] == target.as_str())
        .count();
    assert_eq!(ssh_profiles, 2, "{list}");
    let d = lab.ok(&["new", "prof", "--profile", "lab-plain", "--dry-run"]);
    assert_eq!(d["ssh"]["target"], target.as_str(), "{d}");

    // Nothing anywhere holds the secret: JSON, config, state, the screens.
    let mut seen = String::new();
    for t in ["first", "saved"] {
        seen += &lab.cli(&["info", t]).1.to_string();
        seen += &lab.screen(t);
    }
    seen += &lab.cli(&["ls"]).1.to_string();
    for e in walk(&lab.dir) {
        if e.extension().is_none_or(|x| x != "exe") {
            seen += &String::from_utf8_lossy(&std::fs::read(&e).unwrap_or_default());
        }
    }
    assert!(!seen.contains(&pass), "the passphrase leaked");

    // A wrong saved secret fails once, visibly, without a retry loop.
    let (code, _) = lab.save(&target, &kh, "wrong secret");
    assert_eq!(code, 0);
    lab.ok(&[
        "ssh",
        "connect",
        &target,
        "wrong",
        "--identity",
        &locked_s,
        "--option",
        &kh,
        "--option",
        only,
    ]);
    lab.wait_screen("wrong", "Permission denied");

    // A changed host key is refused with OpenSSH's warning; nothing logs in.
    server.stop();
    keygen(&bin, &host_key, "");
    server.start();
    lab.ok(&[
        "ssh",
        "connect",
        &target,
        "changed",
        "--identity",
        &plain_s,
        "--option",
        &kh,
        "--option",
        only,
    ]);
    let s = lab.wait_screen("changed", "IDENTIFICATION HAS CHANGED");
    assert!(!s.contains("marker-"), "{s}");

    // Offline: the server is gone.
    server.stop();
    lab.ok(&[
        "ssh",
        "connect",
        &target,
        "offline",
        "--identity",
        &plain_s,
        "--option",
        &kh,
    ]);
    lab.wait_screen("offline", "Connection refused");

    // Forget.
    let f = lab.ok(&["ssh", "credential", "forget", &target, "--option", &kh]);
    assert_eq!(f["removed"], true);
    let st = lab.ok(&["ssh", "credential", "status", &target, "--option", &kh]);
    assert_eq!(st["saved"], false);
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

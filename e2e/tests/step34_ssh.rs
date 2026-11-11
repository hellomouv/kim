//! E2E — Step 34: SSH sessions and saved secrets (ADR 0067). `SUBAK-FR-061`, `KIM-SEC-013`.
//!
//! A user connects to a real SSH server (Microsoft's portable sshd from
//! `scripts/fetch-test-sshd.ps1`, run as this user on 127.0.0.1) with the release `subak`:
//! answers the first-trust question, saves the key's passphrase in Credential Manager (only
//! after the host is known), connects again without typing it, and sees a changed host key
//! refused. Windows only; a private endpoint; an `APPDATA` sandbox; the credential is removed.
#![cfg(windows)]
#![allow(unsafe_code)]

use serde_json::Value;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "../../crates/subak/tests/common/hidden.rs"]
mod hidden;
use std::time::{Duration, Instant};

const NO_WINDOW: u32 = 0x0800_0000;

struct Env {
    exe: PathBuf,
    ep: String,
    appdata: PathBuf,
    dir: PathBuf,
    sshd: Option<u32>,
    cred_target: Option<String>,
}

impl Env {
    fn run(&self, args: &[&str], input: Option<&str>) -> (i32, Value) {
        let local = args.starts_with(&["ssh", "credential"]);
        let mut c = Command::new(&self.exe);
        c.arg("--json");
        if !local {
            c.args(["--endpoint", &self.ep]);
        }
        let mut child = c
            .args(args)
            .env("APPDATA", &self.appdata)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(NO_WINDOW)
            .spawn()
            .unwrap();
        if let Some(i) = input {
            writeln!(child.stdin.take().unwrap(), "{i}").unwrap();
        }
        let out = child.wait_with_output().unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code().unwrap_or(-1), doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.run(args, None);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn screen(&self, t: &str) -> String {
        let (_, d) = self.run(&["capture", t], None);
        d["data"]["lines"]
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    fn wait(&self, t: &str, want: &str) -> String {
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

    fn type_line(&self, t: &str, text: &str) {
        self.ok(&["command", "--target", t, &format!("send-keys -l '{text}'")]);
        self.ok(&["command", "--target", t, "send-keys Enter"]);
    }

    /// On a private desktop (Step 35): sshd's session programs may open console windows,
    /// which must never reach the user's screen.
    fn start_sshd(&mut self, bin: &Path) {
        self.sshd = Some(hidden::spawn(
            &bin.join("sshd.exe"),
            &[
                "-D".into(),
                "-f".into(),
                self.dir.join("sshd_config").display().to_string(),
            ],
        ));
        std::thread::sleep(Duration::from_millis(800));
    }

    fn stop_sshd(&mut self) {
        if let Some(pid) = self.sshd.take() {
            hidden::kill_tree(pid);
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.run(&["host", "stop", "--kill-terminals", "--yes"], None);
        if let Some(t) = self.cred_target.take() {
            let _ = self.run(&["ssh", "credential", "forget", &t], None);
        }
        self.stop_sshd();
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn keygen(bin: &Path, file: &Path, pass: &str) {
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_file(file.with_extension("pub"));
    assert!(
        Command::new(bin.join("ssh-keygen.exe"))
            .args(["-q", "-t", "ed25519", "-N", pass, "-f"])
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .creation_flags(NO_WINDOW)
            .status()
            .unwrap()
            .success()
    );
}

fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

fn fwd(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_user_trusts_a_host_saves_a_passphrase_and_a_changed_key_is_refused() {
    let bin = std::env::var_os("SUBAK_TEST_SSHD_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-sshd/OpenSSH-Win64"),
        PathBuf::from,
    );
    assert!(
        bin.join("sshd.exe").is_file(),
        "no test sshd: run scripts/fetch-test-sshd.ps1 first ({})",
        bin.display()
    );
    let id = std::process::id();
    let dir = std::env::temp_dir().join(format!("subak-e2e34-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("appdata")).unwrap();
    let mut env = Env {
        exe: kim_e2e::bin("subak"),
        ep: format!("subak-e2e34-{id}"),
        appdata: dir.join("appdata"),
        dir: dir.clone(),
        sshd: None,
        cred_target: None,
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (host, key) = (dir.join("host_ed25519"), dir.join("user_key"));
    let pass = format!("e2e pass {id}");
    keygen(&bin, &host, "");
    keygen(&bin, &key, &pass);
    std::fs::copy(key.with_extension("pub"), dir.join("authorized_keys")).unwrap();
    std::fs::write(
        dir.join("sshd_config"),
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nStrictModes no\nPidFile {}\nSubsystem sftp none\n",
            fwd(&host),
            fwd(&dir.join("authorized_keys")),
            fwd(&dir.join("sshd.pid"))
        ),
    )
    .unwrap();
    env.start_sshd(&bin);
    let user = std::env::var("USERNAME").unwrap().to_lowercase();
    let target = format!("{user}@127.0.0.1:{port}");
    let kh = format!("UserKnownHostsFile={}", fwd(&dir.join("known_hosts")));
    let key_s = key.display().to_string();
    let conn = |name: &str| -> Vec<String> {
        [
            "ssh",
            "connect",
            &target,
            name,
            "--identity",
            &key_s,
            "--option",
            &kh,
            "--option",
            "IdentitiesOnly=yes",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    };

    // Unknown host: no secret is saved.
    let (code, doc) = env.run(
        &["ssh", "credential", "save", &target, "--option", &kh],
        Some(&pass),
    );
    assert_ne!(code, 0, "{doc}");

    // First trust, answered by the user; the passphrase typed this once.
    let first = conn("first");
    let d = env.ok(&refs(&first));
    assert_eq!(d["ssh"]["host_verified"], false);
    env.wait("first", "continue connecting");
    env.type_line("first", "yes");
    env.wait("first", "passphrase");
    env.type_line("first", &pass);
    std::thread::sleep(Duration::from_millis(1500));
    env.type_line("first", "echo e2e-%OS%");
    env.wait("first", "e2e-Windows_NT");

    // Saved now that the host is known; the next connection needs no typing.
    let (code, doc) = env.run(
        &["ssh", "credential", "save", &target, "--option", &kh],
        Some(&pass),
    );
    assert_eq!(code, 0, "{doc}");
    env.cred_target = Some(target.clone());
    assert!(!doc.to_string().contains(&pass));
    let again = conn("again");
    let d = env.ok(&refs(&again));
    assert_eq!(d["ssh"]["saved_secret_used"], true, "{d}");
    std::thread::sleep(Duration::from_millis(2500));
    env.type_line("again", "echo e2e-%OS%");
    env.wait("again", "e2e-Windows_NT");

    // A new host key: refused.
    env.stop_sshd();
    keygen(&bin, &host, "");
    env.start_sshd(&bin);
    let changed = conn("changed");
    env.ok(&refs(&changed));
    let s = env.wait("changed", "IDENTIFICATION HAS CHANGED");
    assert!(!s.contains("e2e-Windows_NT"), "{s}");
}

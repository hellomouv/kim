//! E2E — Step 33: detected shell profiles and the optional shell integration (ADR 0066).
//! `SUBAK-FR-031`, `KIM-SEC-013`.
//!
//! A user starts Terminals from detected profiles (Command Prompt, Windows PowerShell) with no
//! configuration, sees cmd report its folder after `cd` through a profile `PROMPT`, and installs
//! and removes the PowerShell integration on a scratch profile file, which comes back byte for
//! byte. Windows only; a private endpoint; an `APPDATA` sandbox.
#![cfg(windows)]

use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const LONG: Duration = Duration::from_secs(30);

fn json(exe: &Path, ep: &str, args: &[&str]) -> (i32, Value) {
    let out = Command::new(exe)
        .args(["--json", "--endpoint", ep])
        .args(args)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("run subak");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{args:?}: not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    (out.status.code().unwrap_or(-1), doc)
}

fn ok(exe: &Path, ep: &str, args: &[&str]) -> Value {
    let (code, doc) = json(exe, ep, args);
    assert_eq!(code, 0, "{args:?}: {doc}");
    doc["data"].clone()
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + LONG;
    while !f() {
        assert!(Instant::now() < until, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Cleanup<'a> {
    exe: &'a Path,
    ep: &'a str,
    dir: std::path::PathBuf,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = json(
            self.exe,
            self.ep,
            &["host", "stop", "--kill-terminals", "--yes"],
        );
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn with_appdata(exe: &Path, ep: &str, appdata: &Path, args: &[&str]) -> Value {
    // `profile` and `shell-integration` read files only and take no endpoint.
    let local = matches!(args.first(), Some(&("profile" | "shell-integration")));
    let mut c = Command::new(exe);
    c.arg("--json");
    if !local {
        c.args(["--endpoint", ep]);
    }
    let out = c
        .args(args)
        .env("APPDATA", appdata)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("run subak");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{args:?}: {doc}");
    doc["data"].clone()
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn detected_profiles_start_and_cmd_reports_its_folder() {
    let subak = kim_e2e::bin("subak");
    let ep = format!("subak-e2e33-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("subak-e2e33-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("appdata")).unwrap();
    let appdata = dir.join("appdata");
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
        dir: dir.clone(),
    };
    let list = with_appdata(&subak, &ep, &appdata, &["profile", "list"]);
    let names: Vec<String> = list["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "Command Prompt"), "{names:?}");
    let r = with_appdata(
        &subak,
        &ep,
        &appdata,
        &["new", "c", "--profile", "Command Prompt"],
    );
    assert_eq!(r["profile"], "Command Prompt");
    with_appdata(
        &subak,
        &ep,
        &appdata,
        &["new", "ps", "--profile", "Windows PowerShell"],
    );
    with_appdata(
        &subak,
        &ep,
        &appdata,
        &[
            "new",
            "rep",
            "--env",
            r"PROMPT=$e]9;9;$P$e\$P$G",
            "--",
            "cmd.exe",
            "/d",
        ],
    );
    ok(
        &subak,
        &ep,
        &[
            "command",
            "--target",
            "rep",
            r"send-keys -l 'cd /d C:\Windows'",
        ],
    );
    ok(
        &subak,
        &ep,
        &["command", "--target", "rep", "send-keys Enter"],
    );
    wait_for("cmd reported its folder", || {
        ok(&subak, &ep, &["info", "rep"])["terminal"]["windows"][0]["panes"][0]["current_dir"]
            == r"C:\Windows"
    });

    let ps1 = dir.join("profile.ps1");
    std::fs::write(&ps1, "# mine\r\n").unwrap();
    let f = ps1.to_str().unwrap();
    with_appdata(
        &subak,
        &ep,
        &appdata,
        &["shell-integration", "install", "--profile-file", f],
    );
    assert!(
        std::fs::read_to_string(&ps1)
            .unwrap()
            .contains("subak shell integration")
    );
    with_appdata(
        &subak,
        &ep,
        &appdata,
        &["shell-integration", "uninstall", "--profile-file", f],
    );
    assert_eq!(std::fs::read_to_string(&ps1).unwrap(), "# mine\r\n");
    let _ = json!({});
}

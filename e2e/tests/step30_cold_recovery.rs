//! E2E — Step 30: cold recovery (ADR 0064). `SUBAK-FR-040`, `SUBAK-FR-045`, `SUBAK-SEC-002`.
//!
//! After a Host stop, a user lists what can be recovered, is refused without `--yes`, and
//! recovers a Terminal: the same id, both windows and the split, fresh shells; a job Terminal
//! comes back as a shell (its command is not re-run); a second recover is refused.
//! Windows only; a private endpoint and `--state-dir`.
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

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_user_recovers_a_terminal_without_rerunning_anything() {
    let subak = kim_e2e::bin("subak");
    let ep = format!("subak-e2e30-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("subak-e2e30-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_str().unwrap().to_string();
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
        dir: dir.clone(),
    };
    let marker = dir.join("ran.txt");
    ok(
        &subak,
        &ep,
        &["host", "start", "--no-shell", "--state-dir", &d],
    );
    ok(&subak, &ep, &["new", "work"]);
    ok(
        &subak,
        &ep,
        &[
            "command",
            "--target",
            "work",
            "split-window -h; new-window -n logs",
        ],
    );
    let line = format!(
        "cmd.exe /d /c \"echo ran>\"{}\" & ping -n 600 127.0.0.1 >nul\"",
        marker.display()
    );
    ok(&subak, &ep, &["new", "job", "--command-line", &line]);
    let tid = ok(&subak, &ep, &["info", "work"])["terminal"]["id"].clone();
    wait_for("the job ran once", || marker.exists());
    std::thread::sleep(Duration::from_millis(1000));
    ok(&subak, &ep, &["host", "stop", "--kill-terminals", "--yes"]);
    std::fs::remove_file(&marker).unwrap();

    ok(
        &subak,
        &ep,
        &["host", "start", "--no-shell", "--state-dir", &d],
    );
    let list = ok(&subak, &ep, &["recover", "list"]);
    assert_eq!(list["terminals"].as_array().unwrap().len(), 2);
    let (code, doc) = json(&subak, &ep, &["recover", "work"]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (7, json!("confirmation_required"))
    );
    let r = ok(&subak, &ep, &["recover", "work", "--yes"]);
    assert_eq!(r["terminal_id"], tid);
    let t = ok(&subak, &ep, &["info", "work"])["terminal"].clone();
    assert_eq!(t["windows"].as_array().unwrap().len(), 2);
    assert_eq!(t["windows"][0]["panes"].as_array().unwrap().len(), 2);
    assert_eq!(t["windows"][1]["name"], "logs");
    ok(&subak, &ep, &["recover", "job", "--yes"]);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "the job's command was not run again");
    let (code, doc) = json(&subak, &ep, &["recover", "work", "--yes"]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (7, json!("invalid_state")),
        "{doc}"
    );
}

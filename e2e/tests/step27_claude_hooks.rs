//! E2E — Step 27: Claude Code session events (ADR 0061). `SUBAK-FR-050`, `SUBAK-SEC-002`,
//! `KIM-SEC-013`.
//!
//! A user opts in on a scratch settings file that holds their own hook (the real
//! `~/.claude/settings.json` is never touched), a hook reporter runs inside a real cmd.exe pane
//! the way Claude Code runs it (a **stand-in**: the same stdin JSON and environment, not a
//! Claude process — that needs an account and network), the session shows up for the pane
//! without any prompt text, a forged report from outside the pane is refused, and uninstall
//! gives the user's settings back. Windows only; a private endpoint; everything is stopped.
#![cfg(windows)]

use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const LONG: Duration = Duration::from_secs(30);

fn json(exe: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(exe)
        .arg("--json")
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

fn ok(exe: &Path, args: &[&str]) -> Value {
    let (code, doc) = json(exe, args);
    assert_eq!(code, 0, "{args:?}: {doc}");
    doc["data"].clone()
}

struct Cleanup<'a> {
    exe: &'a Path,
    ep: &'a str,
    dir: PathBuf,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = json(
            self.exe,
            &[
                "--endpoint",
                self.ep,
                "host",
                "stop",
                "--kill-terminals",
                "--yes",
            ],
        );
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_user_opts_in_and_a_pane_reports_its_claude_session_privately() {
    let subak = kim_e2e::bin("subak");
    let ep = format!("subak-e2e27-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("subak-e2e27-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
        dir: dir.clone(),
    };
    // Opt in, on the user's own settings.
    let settings = dir.join("settings.json");
    let mine = "{\n  \"theme\": \"dark\",\n  \"hooks\": {\"Notification\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"beep.cmd\"}]}]}\n}\n";
    std::fs::write(&settings, mine).unwrap();
    let s = settings.to_str().unwrap();
    let d = ok(&subak, &["claude", "hooks", "install", "--settings", s]);
    assert_eq!(d["events"].as_array().unwrap().len(), 6);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(
        v["hooks"]["Notification"][0]["hooks"][0]["command"],
        "beep.cmd"
    );
    let hook = v["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();

    // The hook command, as installed, run in a real pane with a prompt in its input.
    ok(&subak, &["--endpoint", &ep, "new", "work"]);
    let input = dir.join("prompt.json");
    std::fs::write(
        &input,
        json!({"session_id": "e2e-27", "hook_event_name": "UserPromptSubmit", "prompt": "the private plan ghp_E2ESECRETE2ESECRETE2ESECRET12"}).to_string(),
    )
    .unwrap();
    let line = format!("type \"{}\" | {hook}", input.display());
    ok(
        &subak,
        &[
            "--endpoint",
            &ep,
            "command",
            "--target",
            "work",
            &format!("send-keys -l '{line}'"),
        ],
    );
    ok(
        &subak,
        &[
            "--endpoint",
            &ep,
            "command",
            "--target",
            "work",
            "send-keys Enter",
        ],
    );
    let until = Instant::now() + LONG;
    let st = loop {
        let st = ok(&subak, &["--endpoint", &ep, "claude", "status", "work"]);
        if st["panes"].as_array().is_some_and(|p| !p.is_empty()) {
            break st;
        }
        assert!(Instant::now() < until, "no session for the pane");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(st["panes"][0]["state"], "in-progress");
    assert_eq!(st["panes"][0]["session_id"], "e2e-27");
    assert!(
        !st.to_string().contains("SECRET") && !st.to_string().contains("plan"),
        "{st}"
    );

    // A forged report from outside the pane changes nothing.
    let pane = st["panes"][0]["pane_id"].as_str().unwrap().to_string();
    let forged = dir.join("forged.json");
    std::fs::write(
        &forged,
        r#"{"session_id":"e2e-27","hook_event_name":"Stop"}"#,
    )
    .unwrap();
    let out = Command::new(&subak)
        .args(["--json", "claude", "report"])
        .env("SUBAK_PANE_ID", &pane)
        .env("SUBAK_ENDPOINT", &ep)
        .stdin(std::fs::File::open(&forged).unwrap())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["data"]["delivered"], false, "{doc}");
    assert_eq!(
        ok(&subak, &["--endpoint", &ep, "claude", "status", "work"])["panes"][0]["state"],
        "in-progress"
    );

    // Opt out: the user's settings come back.
    ok(&subak, &["claude", "hooks", "uninstall", "--settings", s]);
    let back: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(back, serde_json::from_str::<Value>(mine).unwrap());
}

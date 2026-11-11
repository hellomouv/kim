//! E2E — Step 28: Claude Code status (ADR 0062). `SUBAK-FR-050`, `SUBAK-FR-051`,
//! `SUBAK-FR-052`.
//!
//! Hook reports from inside real cmd.exe panes (a **stand-in for Claude Code**: the installed
//! reporter fed hook JSON, not a Claude process) drive each Terminal's `claude_state`: the five
//! states from their events, the most urgent pane speaking for a Terminal, a plain Terminal
//! staying `none`, `subak claude list` agreeing with `subak ls`. Windows only; private endpoint.
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
fn each_terminal_shows_its_most_urgent_claude_state() {
    let subak = kim_e2e::bin("subak");
    let ep = format!("subak-e2e28-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("subak-e2e28-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
        dir: dir.clone(),
    };
    ok(&subak, &["--endpoint", &ep, "new", "agent"]);
    ok(&subak, &["--endpoint", &ep, "new", "plain"]);
    ok(
        &subak,
        &[
            "--endpoint",
            &ep,
            "command",
            "--target",
            "agent",
            "split-window -h",
        ],
    );
    let t = ok(&subak, &["--endpoint", &ep, "info", "agent"]);
    let panes: Vec<String> = t["terminal"]["windows"][0]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    let state = |name: &str| -> Value {
        ok(&subak, &["--endpoint", &ep, "ls"])["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["claude_state"]
            .clone()
    };
    let mut k = 0;
    let mut hook = |pane: &str, session: &str, event: &str| {
        k += 1;
        let (file, out) = (dir.join(format!("{k}.json")), dir.join(format!("{k}.out")));
        std::fs::write(
            &file,
            json!({"session_id": session, "hook_event_name": event}).to_string(),
        )
        .unwrap();
        let line = format!(
            "type \"{}\" | \"{}\" --json claude report > \"{}\"",
            file.display(),
            subak.display(),
            out.display()
        );
        ok(
            &subak,
            &[
                "--endpoint",
                &ep,
                "command",
                "--target",
                pane,
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
                pane,
                "send-keys Enter",
            ],
        );
        let until = Instant::now() + LONG;
        while !std::fs::read_to_string(&out).is_ok_and(|t| t.contains("\"delivered\":true")) {
            assert!(Instant::now() < until, "the reporter did not answer");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    assert_eq!(state("agent"), "none");
    for (event, want) in [
        ("SessionStart", "idle"),
        ("UserPromptSubmit", "in-progress"),
        ("PermissionRequest", "need-input"),
        ("PostToolUse", "in-progress"),
        ("Stop", "done"),
        ("StopFailure", "error"),
    ] {
        hook(&panes[0], "e1", event);
        assert_eq!(state("agent"), want, "{event}");
    }
    hook(&panes[1], "e2", "PermissionRequest");
    assert_eq!(
        state("agent"),
        "need-input",
        "the more urgent pane speaks for the Terminal"
    );
    assert_eq!(state("plain"), "none", "a Terminal without Claude");
    let list = ok(&subak, &["--endpoint", &ep, "claude", "list", "agent"]);
    assert_eq!(list["panes"].as_array().unwrap().len(), 2);
}

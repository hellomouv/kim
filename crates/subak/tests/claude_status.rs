//! Step 28 — Claude Code status: the state machine and the aggregate per Terminal on a real Host
//! (ADR 0062). `SUBAK-FR-050`, `SUBAK-FR-051`, `SUBAK-FR-052`.
//!
//! Hook reports come from inside real cmd.exe panes (a **stand-in for Claude Code**: the same
//! hook JSON on stdin, the installed reporter command; not a Claude process). The Host runs with
//! `SUBAK_TEST_CLAUDE_STALE_MS` (debug builds only) so a stale signal takes a second, not 30
//! minutes. A raw `gui` client watches the events the window and the floating manager use.
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Lab {
    ep: String,
    host: Proc,
    child: std::process::Child,
    dir: PathBuf,
    n: std::cell::Cell<u32>,
}

impl Lab {
    fn start(tag: &str, stale_ms: &str) -> Self {
        let ep = endpoint(tag);
        // `host serve` as this test's child: `host start` gives the Host the user's logon
        // environment, so the test variable would not reach it.
        use std::os::windows::process::CommandExt;
        let child = std::process::Command::new(subak())
            .args([
                "host",
                "serve",
                "--endpoint",
                &ep,
                "--no-shell",
                "--idle-exit",
                "60",
            ])
            .env("SUBAK_TEST_CLAUDE_STALE_MS", stale_ms)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .expect("host serve");
        let end = Instant::now() + LONG;
        let doc = loop {
            let (code, doc) = run(&["host", "status", "--endpoint", &ep]);
            if code == 0 {
                break doc;
            }
            assert!(Instant::now() < end, "the Host did not come up: {doc}");
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(doc["data"]["host"]["pid"], child.id(), "{doc}");
        let dir = std::env::temp_dir().join(format!("subak-claude28-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            host: Proc::open(pid_of(&doc["data"]["host"]["pid"])),
            child,
            ep,
            dir,
            n: std::cell::Cell::new(0),
        }
    }

    fn cli(&self, args: &[&str]) -> Value {
        let mut a = vec!["--json", "--endpoint", self.ep.as_str()];
        a.extend_from_slice(args);
        let (code, doc) = run_env(&a, &[]);
        validate("subak-cli-v1", &doc);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    /// One hook run inside `pane`, as Claude Code runs it; waits for the reporter's answer.
    fn hook(&self, pane: &str, session: &str, event: &str, extra: Value) -> Value {
        let k = self.n.get() + 1;
        self.n.set(k);
        let mut input = json!({"session_id": session, "hook_event_name": event});
        if let Value::Object(m) = extra {
            for (key, v) in m {
                input[key] = v;
            }
        }
        let (json_file, out) = (
            self.dir.join(format!("{k}.json")),
            self.dir.join(format!("{k}.out")),
        );
        std::fs::write(&json_file, input.to_string()).unwrap();
        let line = format!(
            "type \"{}\" | \"{}\" --json claude report > \"{}\"",
            json_file.display(),
            Path::new(subak()).display(),
            out.display()
        );
        self.cli(&[
            "command",
            "--target",
            pane,
            &format!("send-keys -l '{line}'"),
        ]);
        self.cli(&["command", "--target", pane, "send-keys Enter"]);
        let end = Instant::now() + LONG;
        loop {
            if let Ok(t) = std::fs::read_to_string(&out)
                && let Ok(doc) = serde_json::from_str::<Value>(&t)
            {
                assert_eq!(doc["data"]["delivered"], true, "{doc}");
                return doc["data"].clone();
            }
            assert!(
                Instant::now() < end,
                "the reporter in {pane} did not answer"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The Terminal's `claude_state` as `subak ls` and `subak info` report it.
    fn terminal_state(&self, t: &str) -> Value {
        let info = self.cli(&["info", t])["terminal"]["claude_state"].clone();
        let ls = self.cli(&["ls"])["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == t)
            .unwrap()["claude_state"]
            .clone();
        assert_eq!(info, ls, "ls and info agree");
        info
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = run(&[
            "host",
            "stop",
            "--endpoint",
            &self.ep,
            "--kill-terminals",
            "--yes",
        ]);
        if !self.host.wait(Duration::from_secs(20)) {
            self.host.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Every `terminal.claude_changed` the watcher received so far, as (terminal, state).
fn changes(c: &Raw, seen: &mut Vec<(String, String)>) -> Vec<(String, String)> {
    while let Got::Control(m) = c.next(Duration::from_millis(150)) {
        if m["event"] == "terminal.claude_changed" {
            ipc_valid(&m);
            seen.push((
                m["data"]["terminal_id"].as_str().unwrap().to_string(),
                m["data"]["claude_state"].as_str().unwrap().to_string(),
            ));
        }
    }
    seen.clone()
}

#[test]
fn all_states_aggregate_per_terminal_and_stale_or_lost_signals_become_unknown() {
    let lab = Lab::start("w28-states", "1500");
    lab.cli(&["new", "work"]);
    lab.cli(&["new", "plain"]);
    lab.cli(&["command", "--target", "work", "split-window -h"]);
    let t = lab.cli(&["info", "work"])["terminal"].clone();
    let tid = t["id"].as_str().unwrap().to_string();
    let panes: Vec<String> = t["windows"][0]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    let (a, b) = (&panes[0], &panes[1]);
    let watcher = Raw::handshake(&lab.ep, "gui");
    assert_eq!(
        watcher.request(1, "events.subscribe", &json!({"topics": ["terminals"]}))["ok"],
        true
    );
    let mut seen = Vec::new();

    // Without hooks: `none` everywhere (never a guess); a non-Claude Terminal stays `none`.
    assert_eq!(lab.terminal_state("work"), "none");

    // The five states, from the events that are their evidence.
    for (event, extra, want) in [
        ("SessionStart", json!({"source": "startup"}), "idle"),
        ("UserPromptSubmit", json!({"prompt": "p"}), "in-progress"),
        ("PostToolUse", json!({"tool_name": "Bash"}), "in-progress"),
        (
            "PermissionRequest",
            json!({"tool_name": "Bash"}),
            "need-input",
        ),
        ("PostToolUse", json!({"tool_name": "Bash"}), "in-progress"),
        ("Stop", json!({}), "done"),
        (
            "UserPromptSubmit",
            json!({"prompt": "again"}),
            "in-progress",
        ),
        ("StopFailure", json!({"error": "rate_limit"}), "error"),
    ] {
        let r = lab.hook(a, "s-a", event, extra);
        assert_eq!(r["state"], want, "{event}");
        assert_eq!(
            lab.terminal_state("work"),
            want,
            "{event}: the Terminal follows its only session"
        );
    }
    // A repeated state is no change: no second event.
    let before = changes(&watcher, &mut seen).len();
    lab.hook(a, "s-a", "StopFailure", json!({}));
    assert_eq!(
        changes(&watcher, &mut seen).len(),
        before,
        "a repeated state sends nothing"
    );
    // The events the window sees, in order, with no duplicates.
    let ours: Vec<String> = seen
        .iter()
        .filter(|(t, _)| *t == tid)
        .map(|(_, s)| s.clone())
        .collect();
    assert_eq!(
        ours,
        [
            "idle",
            "in-progress",
            "need-input",
            "in-progress",
            "done",
            "in-progress",
            "error"
        ]
    );

    // Two panes: the more urgent speaks for the Terminal (need-input > error).
    lab.hook(
        b,
        "s-b",
        "Notification",
        json!({"notification_type": "permission_prompt", "message": "m"}),
    );
    assert_eq!(lab.terminal_state("work"), "need-input");
    // `subak claude status` and the Terminal objects agree (what the GUI draws).
    let st = lab.cli(&["claude", "list"]);
    ipc_def("result_claude_status", &st);
    let states: Vec<&str> = st["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["state"].as_str().unwrap())
        .collect();
    assert_eq!(states.len(), 2);
    assert!(
        states.contains(&"error") && states.contains(&"need-input"),
        "{st}"
    );
    assert_eq!(lab.terminal_state("plain"), "none");

    // A lost event: a prompt whose Stop never comes is `in-progress`, then `unknown` once stale
    // — announced without any new hook, so the tab changes by itself.
    lab.hook(b, "s-b", "UserPromptSubmit", json!({"prompt": "p"}));
    lab.hook(a, "s-a", "SessionEnd", json!({"reason": "other"}));
    assert_eq!(lab.terminal_state("work"), "in-progress");
    let end = Instant::now() + LONG;
    while !changes(&watcher, &mut seen)
        .iter()
        .any(|(t, s)| *t == tid && s == "unknown")
    {
        assert!(
            Instant::now() < end,
            "the stale session was never announced"
        );
    }
    assert_eq!(lab.terminal_state("work"), "unknown");
    let st = lab.cli(&["claude", "status", b]);
    assert_eq!(
        (
            st["panes"][0]["state"].clone(),
            st["panes"][0]["stale"].clone()
        ),
        (json!("unknown"), json!(true))
    );
    // A new hook brings back a confirmed state.
    lab.hook(b, "s-b", "Stop", json!({}));
    assert_eq!(lab.terminal_state("work"), "done");
    // The pane ends: its session goes, the Terminal is `none` again.
    lab.cli(&["pane", "kill", b, "--yes"]);
    let end = Instant::now() + LONG;
    while lab.terminal_state("work") != "none" {
        assert!(Instant::now() < end, "the ended pane's state stayed");
        std::thread::sleep(Duration::from_millis(100));
    }
}

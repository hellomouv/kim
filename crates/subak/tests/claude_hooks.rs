//! Step 27 — Claude Code session events (ADR 0010, ADR 0061). `SUBAK-FR-050`, `SUBAK-FR-051`,
//! `SUBAK-SEC-002`, `KIM-SEC-013`.
//!
//! - `subak claude hooks install|uninstall` on a settings file holding the user's own settings
//!   and hooks (a temporary file; the real `~/.claude/settings.json` is never touched).
//! - The reporter inside a real cmd.exe pane, fed hook JSON the way Claude Code feeds it (a
//!   **stand-in for Claude Code**: the same stdin JSON and environment, not a Claude process);
//!   the Host accepts it, tracks several sessions in one pane, ends them, and refuses the same
//!   report from a process outside the pane. Nothing of the hook input but ids, the event and a
//!   coarse reason reaches the Host.
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

fn cli(args: &[&str], env: &[(&str, &str)]) -> (i32, Value) {
    let (code, doc) = run_env(args, env);
    validate("subak-cli-v1", &doc);
    (code, doc)
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subak-claude-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const USER_SETTINGS: &str = "{\r\n  \"model\": \"opus\",\r\n  \"permissions\": {\"allow\": [\"Bash(git status)\"]},\r\n  \"hooks\": {\r\n    \"Stop\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"my-notify.cmd\"}]}]\r\n  }\r\n}\r\n";

#[test]
fn hooks_install_merges_into_the_users_settings_and_uninstall_restores_them() {
    let dir = scratch("settings");
    let file = dir.join("settings.json");
    std::fs::write(&file, USER_SETTINGS).unwrap();
    let f = file.to_str().unwrap();

    let (code, doc) = cli(
        &["--json", "claude", "hooks", "status", "--settings", f],
        &[],
    );
    assert_eq!(code, 0, "{doc}");
    assert_eq!(doc["data"]["installed"], false);
    assert_eq!(doc["data"]["other_hooks"], 1);
    // A dry run changes nothing.
    let (_, doc) = cli(
        &[
            "--json",
            "claude",
            "hooks",
            "install",
            "--settings",
            f,
            "--dry-run",
        ],
        &[],
    );
    assert_eq!(doc["data"]["events"].as_array().unwrap().len(), 8);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), USER_SETTINGS);

    let (code, doc) = cli(
        &["--json", "claude", "hooks", "install", "--settings", f],
        &[],
    );
    assert_eq!(code, 0, "{doc}");
    let backup = PathBuf::from(doc["data"]["backup"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        USER_SETTINGS,
        "a byte-exact backup"
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("\r\n") && !text.replace("\r\n", "").contains('\n'),
        "CRLF kept"
    );
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["model"], "opus");
    assert_eq!(v["permissions"]["allow"][0], "Bash(git status)");
    assert_eq!(
        v["hooks"]["Stop"][0]["hooks"][0]["command"], "my-notify.cmd",
        "the user's hook stays first"
    );
    let ours = v["hooks"]["Stop"][1]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(ours.ends_with("subak.exe\" claude report"), "{ours}");
    // Installing again changes nothing and writes no second backup.
    let (_, doc) = cli(
        &["--json", "claude", "hooks", "install", "--settings", f],
        &[],
    );
    assert_eq!(doc["data"]["events"], json!([]));
    assert!(doc["data"]["backup"].is_null());

    let (code, doc) = cli(
        &["--json", "claude", "hooks", "uninstall", "--settings", f],
        &[],
    );
    assert_eq!(code, 0, "{doc}");
    assert_ne!(
        doc["data"]["backup"],
        backup.to_str().unwrap(),
        "never over an earlier backup"
    );
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), USER_SETTINGS);
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        after,
        serde_json::from_str::<Value>(USER_SETTINGS).unwrap(),
        "exactly the user's settings again"
    );

    // An invalid file is refused and left alone.
    std::fs::write(&file, "{ \"model\": ").unwrap();
    let (code, doc) = cli(
        &["--json", "claude", "hooks", "install", "--settings", f],
        &[],
    );
    assert_ne!(code, 0);
    assert_eq!(doc["error"]["code"], "config_invalid", "{doc}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ \"model\": ");
    // A missing file (and folder) is created with just the hooks.
    let fresh = dir.join("new").join("settings.json");
    let (code, _) = cli(
        &[
            "--json",
            "claude",
            "hooks",
            "install",
            "--settings",
            fresh.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(code, 0);
    assert!(
        std::fs::read_to_string(&fresh)
            .unwrap()
            .contains("SessionStart")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

struct Lab {
    h: Host,
    dir: PathBuf,
}

impl Lab {
    fn cli(&self, args: &[&str]) -> Value {
        let mut a = vec!["--json", "--endpoint", self.h.ep.as_str()];
        a.extend_from_slice(args);
        let (code, doc) = cli(&a, &[]);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn status(&self) -> Value {
        let d = self.cli(&["claude", "status"]);
        ipc_def("result_claude_status", &d);
        d
    }

    /// Types `type <json file> | subak claude report --json > <out>` into the pane, as a Claude
    /// Code hook would run it, and returns the reporter's answer.
    fn hook(&self, pane: &str, name: &str, input: &Value) -> Value {
        let json_file = self.dir.join(format!("{name}.json"));
        let out = self.dir.join(format!("{name}.out"));
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
                validate("subak-cli-v1", &doc);
                return doc["data"].clone();
            }
            assert!(
                Instant::now() < end,
                "the reporter in {pane} did not answer"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
fn reports_from_inside_a_pane_track_sessions_and_outsiders_are_refused() {
    let lab = Lab {
        h: Host::start("w27-claude", &["--no-shell", "--idle-exit", "60"]),
        dir: scratch("lab"),
    };
    lab.cli(&["new", "work"]);
    lab.cli(&["command", "--target", "work", "split-window -h"]);
    let t = lab.cli(&["pane", "ls", "work"]);
    let panes: Vec<String> = t["windows"][0]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    let (a, b) = (&panes[0], &panes[1]);
    assert_eq!(
        lab.status()["panes"],
        json!([]),
        "no hooks, no sessions: none, never a guess"
    );

    // A session starts and a prompt is submitted: the prompt's text goes nowhere.
    let r = lab.hook(a, "start", &json!({"session_id": "sess-1", "hook_event_name": "SessionStart", "source": "startup", "cwd": "C:/w", "transcript_path": "C:/secret/t.jsonl"}));
    assert_eq!(
        (r["delivered"].clone(), r["state"].clone()),
        (json!(true), json!("idle")),
        "{r}"
    );
    assert_eq!(
        r["pane_id"],
        a.as_str(),
        "the pane comes from SUBAK_PANE_ID"
    );
    lab.hook(a, "prompt", &json!({"session_id": "sess-1", "hook_event_name": "UserPromptSubmit", "prompt": "deploy with token ghp_SECRETSECRETSECRETSECRET1234"}));
    let st = lab.status();
    assert_eq!(st["panes"][0]["state"], "in-progress");
    assert!(
        !st.to_string().contains("SECRET") && !st.to_string().contains("secret"),
        "{st}"
    );
    // A permission prompt waits for the user; a second Claude in the same pane is its own session.
    lab.hook(a, "perm", &json!({"session_id": "sess-1", "hook_event_name": "Notification", "notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"}));
    lab.hook(
        a,
        "second",
        &json!({"session_id": "sess-2", "hook_event_name": "SessionStart", "source": "startup"}),
    );
    lab.hook(
        b,
        "other-pane",
        &json!({"session_id": "sess-3", "hook_event_name": "UserPromptSubmit", "prompt": "x"}),
    );
    let st = lab.status();
    let pa = st["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pane_id"] == a.as_str())
        .unwrap()
        .clone();
    assert_eq!(pa["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(
        pa["session_id"], "sess-2",
        "the latest session speaks for the pane"
    );
    let s1 = pa["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == "sess-1")
        .unwrap()
        .clone();
    assert_eq!(
        (s1["state"].clone(), s1["reason"].clone()),
        (json!("need-input"), json!("permission"))
    );
    assert_eq!(
        lab.cli(&["claude", "status", b])["panes"][0]["state"],
        "in-progress",
        "a target narrows it"
    );
    // An event Subak does not know (a newer Claude Code) keeps the state; Stop is done.
    lab.hook(
        a,
        "future",
        &json!({"session_id": "sess-1", "hook_event_name": "SomeFutureEvent"}),
    );
    lab.hook(
        a,
        "stop",
        &json!({"session_id": "sess-2", "hook_event_name": "Stop", "stop_hook_active": false}),
    );
    let st = lab.cli(&["claude", "status", a]);
    let s = |id: &str| {
        st["panes"][0]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["session_id"] == id)
            .unwrap()["state"]
            .clone()
    };
    assert_eq!(
        (s("sess-1"), s("sess-2")),
        (json!("need-input"), json!("done"))
    );
    // SessionEnd ends one session.
    lab.hook(a, "end", &json!({"session_id": "sess-2", "hook_event_name": "SessionEnd", "reason": "prompt_input_exit"}));
    assert_eq!(
        lab.cli(&["claude", "status", a])["panes"][0]["sessions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // The same report from outside the pane (this test process, claiming the pane) is refused.
    let input =
        std::env::temp_dir().join(format!("subak-claude-forged-{}.json", std::process::id()));
    std::fs::write(
        &input,
        r#"{"session_id":"forged","hook_event_name":"Stop"}"#,
    )
    .unwrap();
    let out = std::process::Command::new(subak())
        .args(["--json", "claude", "report"])
        .env("SUBAK_PANE_ID", a)
        .env("SUBAK_ENDPOINT", &lab.h.ep)
        .stdin(std::fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    assert!(out.status.success(), "the reporter never fails");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    validate("subak-cli-v1", &doc);
    assert_eq!(doc["data"]["delivered"], false, "{doc}");
    assert!(
        doc["data"]["why"]
            .as_str()
            .unwrap()
            .contains("running in it"),
        "{doc}"
    );
    assert!(!lab.status().to_string().contains("forged"));
    let stats = lab.h.stats();
    assert_eq!(stats["ipc"]["claude_refused"], 1, "{stats}");
    // Without --json the reporter prints nothing at all (Claude would read it).
    let quiet = std::process::Command::new(subak())
        .args(["claude", "report"])
        .env("SUBAK_PANE_ID", a)
        .env("SUBAK_ENDPOINT", &lab.h.ep)
        .stdin(std::fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    assert!(quiet.status.success() && quiet.stdout.is_empty());
    let _ = std::fs::remove_file(&input);

    // A pane that ends takes its sessions with it.
    lab.cli(&["pane", "kill", b, "--yes"]);
    let end = Instant::now() + LONG;
    while lab.status()["panes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["pane_id"] == b.as_str())
    {
        assert!(Instant::now() < end, "the ended pane's session stayed");
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

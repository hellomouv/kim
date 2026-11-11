//! Step 30 — cold recovery and Claude resume decisions (ADR 0064). `SUBAK-FR-040`,
//! `SUBAK-FR-045`, `SUBAK-SEC-002`.
//!
//! Real Hosts (`host serve` as this test's child, so `CLAUDE_CONFIG_DIR` points at a scratch
//! folder) with a private endpoint and `--state-dir`. Claude resume is checked by its plan —
//! a session found, missing or already running — never by starting Claude Code here (a real
//! resume is manual evidence in the Step 30 report).
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Lab {
    ep: String,
    dir: PathBuf,
    claude_dir: PathBuf,
    child: Option<std::process::Child>,
    n: u32,
}

impl Lab {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("subak-rec30-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("state")).unwrap();
        let claude_dir = dir.join("claude");
        std::fs::create_dir_all(claude_dir.join("projects").join("C--work")).unwrap();
        Self {
            ep: endpoint(tag),
            dir,
            claude_dir,
            child: None,
            n: 0,
        }
    }

    fn sd(&self) -> String {
        self.dir.join("state").display().to_string()
    }

    fn start(&mut self) {
        let child = std::process::Command::new(subak())
            .args([
                "host",
                "serve",
                "--endpoint",
                &self.ep,
                "--no-shell",
                "--idle-exit",
                "60",
                "--state-dir",
                &self.sd(),
            ])
            .env("CLAUDE_CONFIG_DIR", &self.claude_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let end = Instant::now() + LONG;
        while run(&["host", "status", "--endpoint", &self.ep]).0 != 0 {
            assert!(Instant::now() < end, "the Host did not come up");
            std::thread::sleep(Duration::from_millis(50));
        }
        self.child = Some(child);
    }

    fn stop(&mut self) {
        let (code, doc) = run(&[
            "host",
            "stop",
            "--endpoint",
            &self.ep,
            "--kill-terminals",
            "--yes",
        ]);
        assert_eq!(code, 0, "{doc}");
        if let Some(mut c) = self.child.take() {
            let _ = c.wait();
        }
    }

    fn cli(&self, args: &[&str]) -> (i32, Value) {
        let mut a = vec!["--json", "--endpoint", self.ep.as_str()];
        a.extend_from_slice(args);
        let (code, doc) = run_env(&a, &[]);
        validate("subak-cli-v1", &doc);
        (code, doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn wait_saved(&self, what: &str, f: impl Fn(&[Value]) -> bool) {
        let end = Instant::now() + LONG;
        loop {
            if std::fs::read_to_string(self.dir.join("state").join("state.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .is_some_and(|v| f(v["terminals"].as_array().unwrap()))
            {
                return;
            }
            assert!(Instant::now() < end, "state.json never showed {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// A Claude Code hook report from inside `pane` (a stand-in for Claude, as in Step 27).
    fn hook(&mut self, pane: &str, session: &str, event: &str) {
        self.n += 1;
        let (file, out) = (
            self.dir.join(format!("{}.json", self.n)),
            self.dir.join(format!("{}.out", self.n)),
        );
        std::fs::write(
            &file,
            json!({"session_id": session, "hook_event_name": event}).to_string(),
        )
        .unwrap();
        let line = format!(
            "type \"{}\" | \"{}\" --json claude report > \"{}\"",
            file.display(),
            Path::new(subak()).display(),
            out.display()
        );
        self.ok(&[
            "command",
            "--target",
            pane,
            &format!("send-keys -l '{line}'"),
        ]);
        self.ok(&["command", "--target", pane, "send-keys Enter"]);
        let end = Instant::now() + LONG;
        while !std::fs::read_to_string(&out).is_ok_and(|t| t.contains("\"delivered\":true")) {
            assert!(Instant::now() < end, "the reporter did not answer");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = run(&[
                "host",
                "stop",
                "--endpoint",
                &self.ep,
                "--kill-terminals",
                "--yes",
            ]);
            if let Some(mut c) = self.child.take() {
                let _ = c.wait();
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn layout_shape(l: &Value) -> Value {
    match l["type"].as_str() {
        Some("pane") => json!("pane"),
        _ => json!({"d": l["direction"], "c": l["children"].as_array().unwrap().iter()
            .map(|c| json!([c["cells"], layout_shape(&c["node"])])).collect::<Vec<_>>()}),
    }
}

/// The saved topology comes back with its id, names, layout tree and sizes; only shells start;
/// commands run again only on explicit request; a second restore and an unconfirmed one are
/// refused; `auto` restores at the next start, shells only.
#[test]
fn a_saved_terminal_comes_back_as_it_was_without_rerunning_anything() {
    let mut lab = Lab::new("w30-topo");
    lab.start();
    let marker = lab.dir.join("ran.txt");
    let line = format!(
        "cmd.exe /d /c \"echo ran>>\"{}\" & ping -n 600 127.0.0.1 >nul\"",
        marker.display()
    );
    lab.ok(&["new", "work", "--size", "120x40"]);
    lab.ok(&[
        "command",
        "--target",
        "work",
        "split-window -h; split-window -v; resize-pane -L 7; new-window -n logs",
    ]);
    lab.ok(&["new", "job", "--command-line", &line]);
    let before = lab.ok(&["pane", "ls", "work"]);
    let tid = before["terminal_id"].as_str().unwrap().to_string();
    let wait = Instant::now() + LONG;
    while !marker.exists() {
        assert!(Instant::now() < wait, "the job did not run once");
        std::thread::sleep(Duration::from_millis(100));
    }
    lab.wait_saved("both Terminals", |t| t.len() == 2);
    std::thread::sleep(Duration::from_millis(700));
    lab.stop();
    std::fs::remove_file(&marker).unwrap();

    lab.start();
    let list = lab.ok(&["recover", "list"]);
    assert_eq!(list["terminals"].as_array().unwrap().len(), 2);
    assert!(
        list["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["state"] == "restorable")
    );
    assert!(
        list["terminals"][0]["not_restored"]
            .as_array()
            .unwrap()
            .len()
            >= 3,
        "honest about what is lost"
    );
    // Unconfirmed and dry runs start nothing.
    let (code, doc) = lab.cli(&["recover", "work"]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (7, json!("confirmation_required"))
    );
    let plan = lab.ok(&["recover", "job", "--dry-run"]);
    let pane = &plan["would_restore"]["panes"][0];
    assert_eq!(
        pane["launch"], "shell",
        "the saved command is not run by default"
    );
    assert!(pane["notes"].to_string().contains("--with-commands"));
    assert_eq!(lab.ok(&["ls"])["terminals"], json!([]));
    // Restore: the same id, windows, names and layout sizes; new pane ids.
    let r = lab.ok(&["recover", "work", "--yes"]);
    assert_eq!(r["terminal_id"], tid.as_str());
    assert_eq!(r["failed"], json!([]));
    let after = lab.ok(&["pane", "ls", "work"]);
    assert_eq!(after["terminal_id"], tid.as_str());
    let w = |v: &Value| v["windows"].as_array().unwrap().clone();
    assert_eq!(w(&after).len(), 2);
    assert_eq!(w(&after)[1]["name"], "logs");
    assert_eq!(
        layout_shape(&w(&after)[0]["layout"]),
        layout_shape(&w(&before)[0]["layout"]),
        "the same tree and sizes"
    );
    let old: Vec<Value> = w(&before)
        .iter()
        .flat_map(|x| x["panes"].as_array().unwrap().clone())
        .map(|p| p["id"].clone())
        .collect();
    for p in w(&after)
        .iter()
        .flat_map(|x| x["panes"].as_array().unwrap().clone())
    {
        assert!(!old.contains(&p["id"]), "a new process gets a new pane id");
        assert_eq!(p["running"], true);
    }
    // Twice: refused.
    let (code, doc) = lab.cli(&["recover", "work", "--yes"]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (7, json!("invalid_state")),
        "{doc}"
    );
    // The job: a shell by default; its command only with --with-commands.
    lab.ok(&["recover", "job", "--yes"]);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "recovering never re-ran the job");
    lab.ok(&["kill", "job", "--yes"]);
    lab.wait_saved("job gone", |t| t.len() == 1);

    // auto: comes back at the next start, as a shell.
    lab.ok(&["state", "policy", "work", "auto"]);
    std::thread::sleep(Duration::from_millis(800));
    lab.stop();
    lab.start();
    let end = Instant::now() + LONG;
    while lab.ok(&["ls"])["terminals"].as_array().unwrap().is_empty() {
        assert!(Instant::now() < end, "an auto Terminal did not come back");
        std::thread::sleep(Duration::from_millis(100));
    }
    let t = lab.ok(&["info", "work"])["terminal"].clone();
    assert_eq!(t["id"], tid.as_str());
    assert_eq!(t["windows"].as_array().unwrap().len(), 2);
}

/// Resume decisions: a found session resumes (planned as `claude --resume`), a missing one
/// starts a shell with a note, one already running elsewhere is never started twice, and
/// nothing resumes without the opt-in.
#[test]
fn claude_sessions_resume_once_only_when_found_and_asked() {
    let mut lab = Lab::new("w30-claude");
    lab.start();
    lab.ok(&["new", "agent"]);
    lab.ok(&["new", "gone"]);
    let pane_of = |lab: &Lab, t: &str| {
        lab.ok(&["pane", "ls", t])["windows"][0]["panes"][0]["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (a, g) = (pane_of(&lab, "agent"), pane_of(&lab, "gone"));
    let sid = "0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f0";
    let missing = "99999999-0000-4000-8000-000000000000";
    lab.hook(&a, sid, "UserPromptSubmit");
    lab.hook(&g, missing, "UserPromptSubmit");
    // The session ended cleanly: its id is still the pane's last one (resumable).
    lab.hook(&a, sid, "SessionEnd");
    lab.wait_saved("both session ids", |t| {
        t.iter()
            .all(|t| t["windows"][0]["panes"][0]["claude"]["session_id"].is_string())
    });
    lab.stop();
    std::fs::write(
        lab.claude_dir
            .join("projects")
            .join("C--work")
            .join(format!("{sid}.jsonl")),
        "{}\n",
    )
    .unwrap();

    lab.start();
    let plan = |lab: &Lab, t: &str, resume: bool| {
        let mut args = vec!["recover", t, "--dry-run"];
        if resume {
            args.push("--claude-resume");
        }
        lab.ok(&args)["would_restore"]["panes"][0].clone()
    };
    let p = plan(&lab, "agent", false);
    assert_eq!(p["launch"], "shell", "no resume without the opt-in");
    assert_eq!(p["claude"]["found"], true);
    let p = plan(&lab, "agent", true);
    assert_eq!(p["launch"], "claude", "{p}");
    let p = plan(&lab, "gone", true);
    assert_eq!(
        (p["launch"].clone(), p["claude"]["found"].clone()),
        (json!("shell"), json!(false))
    );
    assert!(p["notes"].to_string().contains("not found"), "{p}");
    // The same session already runs in another pane: never twice.
    lab.ok(&["new", "elsewhere"]);
    let e = pane_of(&lab, "elsewhere");
    lab.hook(&e, sid, "SessionStart");
    let p = plan(&lab, "agent", true);
    assert_eq!(p["launch"], "shell", "{p}");
    assert!(
        p["claude"]["running_in"].as_str() == Some(e.as_str()),
        "{p}"
    );
    // The opt-in can be stored with the policy (then an auto restore may resume it).
    lab.ok(&["state", "policy", "agent", "ask", "--claude-resume"]);
    lab.wait_saved("resume on", |t| {
        t.iter().any(|t| {
            t["name"] == "agent" && t["windows"][0]["panes"][0]["claude"]["resume"] == true
        })
    });
    // Restoring `gone` starts a shell in place of the missing session.
    let r = lab.ok(&["recover", "gone", "--claude-resume", "--yes"]);
    assert_eq!(r["failed"], json!([]));
    let pid = lab.ok(&["pane", "ls", "gone"])["windows"][0]["panes"][0]["pid"].clone();
    assert!(pid.as_u64().is_some());
}

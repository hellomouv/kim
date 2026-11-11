//! Step 29 — durable Terminal state (ADR 0006, ADR 0063). `SUBAK-FR-040`, `SUBAK-FR-046`,
//! `SUBAK-SEC-001`, `SUBAK-SEC-002`, `SUBAK-SEC-003`, `SUBAK-SEC-011`.
//!
//! Real `subak.exe` Hosts with a private endpoint and a temporary `--state-dir` (the user's own
//! state is never touched). Every `--json` document is validated against `subak-cli-v1`, every
//! `state.json` against `subak-state-v2`.
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
    dir: PathBuf,
    host: Option<Proc>,
}

impl Lab {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("subak-state29-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            ep: endpoint(tag),
            dir,
            host: None,
        }
    }

    fn d(&self) -> &str {
        self.dir.to_str().unwrap()
    }

    fn file(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn start(&mut self) {
        let (code, doc) = run(&[
            "host",
            "start",
            "--endpoint",
            &self.ep,
            "--no-shell",
            "--idle-exit",
            "60",
            "--state-dir",
            self.d(),
        ]);
        assert_eq!(code, 0, "{doc}");
        self.host = Some(Proc::open(pid_of(&doc["data"]["host"]["pid"])));
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
        if let Some(h) = self.host.take() {
            assert!(h.wait(Duration::from_secs(20)));
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

    fn state(&self) -> Value {
        let t = std::fs::read_to_string(self.file()).expect("state.json");
        let v: Value = serde_json::from_str(&t).unwrap();
        validate("subak-state-v2", &v);
        v
    }

    fn status(&self) -> Value {
        let c = Raw::handshake(&self.ep, "cli");
        let r = c.request(1, "state.status", &json!({}));
        assert_eq!(r["ok"], true, "{r}");
        r["result"].clone()
    }

    fn written(&self, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + LONG;
        loop {
            if let Ok(t) = std::fs::read_to_string(self.file())
                && let Ok(v) = serde_json::from_str::<Value>(&t)
                && f(&v)
            {
                validate("subak-state-v2", &v);
                return v;
            }
            assert!(Instant::now() < end, "state.json never showed {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        if let Some(h) = &self.host
            && h.alive()
        {
            let _ = run(&[
                "host",
                "stop",
                "--endpoint",
                &self.ep,
                "--kill-terminals",
                "--yes",
            ]);
            if !h.wait(Duration::from_secs(20)) {
                h.kill();
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn names(v: &Value) -> Vec<String> {
    v["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

/// Topology survives a clean stop; a restart keeps it as saved records and starts nothing,
/// not even a command that leaves a mark; the policy and forgetting are explicit.
#[test]
fn topology_survives_a_clean_stop_and_loading_it_starts_nothing() {
    let mut lab = Lab::new("w29-clean");
    lab.start();
    let marker = lab.dir.join("ran.txt");
    let line = format!(
        "cmd.exe /d /c \"echo ran>\"{}\" & ping -n 600 127.0.0.1 >nul\"",
        marker.display()
    );
    lab.ok(&["new", "work"]);
    lab.ok(&["new", "marker", "--command-line", &line]);
    lab.ok(&[
        "command",
        "--target",
        "work",
        "split-window -h; new-window -n logs; select-layout -t work:0 main-vertical",
    ]);
    let v = lab.written("both Terminals and the split", |v| {
        v["terminals"].as_array().is_some_and(|t| t.len() == 2)
            && v.to_string().contains("\"logs\"")
    });
    assert_eq!(v["schema_version"], 2);
    assert!(!v.to_string().contains("\"pid\""), "no PIDs are stored");
    let wait = Instant::now() + LONG;
    while !marker.exists() {
        assert!(Instant::now() < wait, "the marker program did not run once");
        std::thread::sleep(Duration::from_millis(100));
    }
    lab.stop();
    let after = lab.state();
    let mut n = names(&after);
    n.sort();
    assert_eq!(
        n,
        ["marker", "work"],
        "a clean stop keeps the Terminals it ended"
    );
    let work = after["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "work")
        .unwrap()
        .clone();
    assert_eq!(work["windows"].as_array().unwrap().len(), 2);
    assert_eq!(work["windows"][0]["panes"].as_array().unwrap().len(), 2);
    assert_eq!(work["windows"][0]["layout"]["type"], "split");

    // Restart: nothing runs, the records stay, `subak state show` lists them.
    std::fs::remove_file(&marker).unwrap();
    lab.start();
    assert_eq!(
        lab.ok(&["ls"])["terminals"],
        json!([]),
        "no Terminal was started"
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "loading state never runs a saved command");
    let st = lab.status();
    assert_eq!(st["saved"].as_array().unwrap().len(), 2, "{st}");
    let show = lab.ok(&["state", "show", "--state-dir", lab.d()]);
    assert_eq!(show["terminals"].as_array().unwrap().len(), 2);
    // A new Terminal joins the saved ones in the file; the saved ones are kept as they were.
    lab.ok(&["new", "third"]);
    let v = lab.written("three Terminals", |v| {
        v["terminals"].as_array().is_some_and(|t| t.len() == 3)
    });
    assert!(names(&v).contains(&"work".to_string()));
    // Policy and forgetting go through the running Host; dry runs change nothing.
    let r = lab.ok(&["state", "policy", "work", "auto", "--state-dir", lab.d()]);
    assert_eq!(r["via"], "host");
    lab.written("work restore auto", |v| {
        v.to_string().contains("\"restore\":\"auto\"")
            || v["terminals"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == "work" && t["restore"] == "auto")
    });
    let r = lab.ok(&[
        "state",
        "forget",
        "marker",
        "--dry-run",
        "--state-dir",
        lab.d(),
    ]);
    assert_eq!(r["result"]["dry_run"], true);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(names(&lab.state()).len(), 3);
    lab.ok(&["state", "forget", "marker", "--state-dir", lab.d()]);
    lab.written("marker forgotten", |v| {
        !names(v).contains(&"marker".to_string())
    });
    let (code, doc) = lab.cli(&["state", "forget", "nope", "--state-dir", lab.d()]);
    assert_ne!(code, 0, "{doc}");
    lab.stop();
    // Without a Host the CLI edits the file itself (backup first).
    let before = lab.ok(&["state", "backups", "--state-dir", lab.d()])["backups"]
        .as_array()
        .unwrap()
        .len();
    let r = lab.ok(&["state", "policy", "work", "never", "--state-dir", lab.d()]);
    assert_eq!(r["via"], "file");
    assert!(
        lab.state()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "work" && t["restore"] == "never")
    );
    assert!(
        lab.ok(&["state", "backups", "--state-dir", lab.d()])["backups"]
            .as_array()
            .unwrap()
            .len()
            >= before.min(4)
    );
}

/// A Host killed while it writes leaves a valid file; a torn file is set aside and the newest
/// backup is used; a newer version's file is never rewritten; rollback restores a backup.
#[test]
fn crashes_corruption_newer_versions_and_rollback_never_lose_a_good_state() {
    let mut lab = Lab::new("w29-crash");
    lab.start();
    lab.ok(&["new", "keep"]);
    lab.written("keep", |v| names(v) == ["keep"]);
    // A burst of changes, then the Host is killed (no clean stop): the file is old or new,
    // never torn.
    for i in 0..6 {
        lab.ok(&[
            "command",
            "--target",
            "keep",
            &format!("new-window -n w{i}"),
        ]);
    }
    lab.host.as_ref().unwrap().kill();
    lab.host = None;
    std::thread::sleep(Duration::from_millis(300));
    let (code, doc) = lab.cli(&["state", "check", "--state-dir", lab.d()]);
    assert_eq!(code, 0, "after a kill: {doc}");
    assert_eq!(names(&lab.state()), ["keep"]);

    // A torn state.json: `check` says so; the next Host keeps it aside and loads a backup.
    lab.start(); // makes a start backup of the good file
    lab.stop();
    let good = std::fs::read_to_string(lab.file()).unwrap();
    std::fs::write(lab.file(), &good[..good.len() / 3]).unwrap();
    let (code, doc) = lab.cli(&["state", "check", "--state-dir", lab.d()]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (4, json!("state_corrupt")),
        "{doc}"
    );
    lab.start();
    let st = lab.status();
    assert!(st["loaded"]["recovered_from"].is_string(), "{st}");
    assert!(
        st["loaded"]["corrupt_kept"]
            .as_str()
            .is_some_and(|p| Path::new(p).exists()),
        "{st}"
    );
    assert_eq!(st["saved"][0]["name"], "keep");
    lab.stop();
    assert_eq!(
        names(&lab.state()),
        ["keep"],
        "the recovered state was written back"
    );

    // A newer version: read-only for Host and CLI, the bytes never change.
    let mut newer: Value =
        serde_json::from_str(&std::fs::read_to_string(lab.file()).unwrap()).unwrap();
    newer["schema_version"] = json!(99);
    let mut body = newer.clone();
    body.as_object_mut().unwrap().remove("crc32");
    let canonical = serde_json::to_string(&body).unwrap();
    newer["crc32"] = json!(format!("{:08x}", crc32(canonical.as_bytes())));
    let text = serde_json::to_string_pretty(&newer).unwrap();
    std::fs::write(lab.file(), &text).unwrap();
    let (code, doc) = lab.cli(&["state", "check", "--state-dir", lab.d()]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (5, json!("state_from_newer_version")),
        "{doc}"
    );
    lab.start();
    assert!(lab.status()["read_only"].is_string());
    lab.ok(&["new", "later"]);
    std::thread::sleep(Duration::from_millis(1200));
    lab.stop();
    assert_eq!(
        std::fs::read_to_string(lab.file()).unwrap(),
        text,
        "a newer file is never rewritten"
    );

    // Rollback: refused while a Host runs; then the newest valid backup comes back.
    lab.start();
    let (code, doc) = lab.cli(&["state", "rollback", "--state-dir", lab.d()]);
    assert_eq!(doc["error"]["code"], "host_already_running", "{code} {doc}");
    lab.stop();
    let r = lab.ok(&["state", "rollback", "--dry-run", "--state-dir", lab.d()]);
    assert_eq!(r["dry_run"], true);
    assert_eq!(std::fs::read_to_string(lab.file()).unwrap(), text);
    lab.ok(&["state", "rollback", "--state-dir", lab.d()]);
    assert_eq!(names(&lab.state()), ["keep"]);
    assert_eq!(lab.state()["schema_version"], 2);
}

/// Secret-looking commands are not stored; a private endpoint without --state-dir writes
/// nothing; missing working directories are reported, not acted on.
#[test]
fn secrets_are_withheld_private_hosts_write_nothing_and_missing_paths_are_reported() {
    let mut lab = Lab::new("w29-secret");
    lab.start();
    let token = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let gone = lab.dir.join("gone-dir");
    std::fs::create_dir_all(&gone).unwrap();
    lab.ok(&[
        "new",
        "tok",
        "--cwd",
        gone.to_str().unwrap(),
        "--",
        "cmd.exe",
        "/d",
        "/k",
        &format!("set TOKEN={token}"),
    ]);
    let v = lab.written("tok", |v| names(v) == ["tok"]);
    let s = v.to_string();
    assert!(!s.contains(token), "a secret never reaches state.json");
    let p = &v["terminals"][0]["windows"][0]["panes"][0];
    assert_eq!(
        (p["command"].clone(), p["command_withheld"].clone()),
        (Value::Null, json!(true))
    );
    lab.stop();
    std::fs::remove_dir_all(&gone).unwrap();
    let show = lab.ok(&["state", "show", "--state-dir", lab.d()]);
    assert_eq!(
        show["terminals"][0]["missing"][0]["missing"], "cwd",
        "{show}"
    );

    // A private endpoint without --state-dir: no state at all.
    let ep = endpoint("w29-nostate");
    let (code, doc) = run(&[
        "host",
        "start",
        "--endpoint",
        &ep,
        "--no-shell",
        "--idle-exit",
        "60",
    ]);
    assert_eq!(code, 0, "{doc}");
    let h = Proc::open(pid_of(&doc["data"]["host"]["pid"]));
    let c = Raw::handshake(&ep, "cli");
    let r = c.request(1, "state.status", &json!({}));
    assert_eq!(r["result"]["enabled"], false, "{r}");
    let _ = run(&["host", "stop", "--endpoint", &ep, "--yes"]);
    assert!(h.wait(Duration::from_secs(20)));
}

/// CRC-32 (IEEE), as the Host computes it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for b in bytes {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

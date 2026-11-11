//! E2E — Step 29: durable Terminal state (ADR 0063). `SUBAK-FR-040`, `SUBAK-SEC-002`,
//! `SUBAK-SEC-003`.
//!
//! A user's Terminal topology (two Terminals, a split, a second window) survives a clean Host
//! stop in a private `--state-dir`; the next Host starts nothing — not even the saved command
//! that leaves a mark — and `subak state show` lists the saved records; a torn file is reported
//! by `subak state check`, set aside by the next Host and replaced from a backup. Windows only.
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
fn the_topology_survives_a_stop_and_a_restart_starts_nothing() {
    let subak = kim_e2e::bin("subak");
    let ep = format!("subak-e2e29-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("subak-e2e29-{}", std::process::id()));
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
    let line = format!(
        "cmd.exe /d /c \"echo ran>\"{}\" & ping -n 600 127.0.0.1 >nul\"",
        marker.display()
    );
    ok(&subak, &ep, &["new", "marker", "--command-line", &line]);
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
    wait_for("the marker ran once", || marker.exists());
    std::thread::sleep(Duration::from_millis(1000));
    ok(&subak, &ep, &["host", "stop", "--kill-terminals", "--yes"]);
    let show = ok(&subak, &ep, &["state", "show", "--state-dir", &d]);
    let saved = show["terminals"].as_array().unwrap();
    assert_eq!(saved.len(), 2, "{show}");
    let work = saved.iter().find(|t| t["name"] == "work").unwrap();
    assert_eq!(
        (work["windows"].clone(), work["panes"].clone()),
        (2.into(), 3.into())
    );

    std::fs::remove_file(&marker).unwrap();
    ok(
        &subak,
        &ep,
        &["host", "start", "--no-shell", "--state-dir", &d],
    );
    assert_eq!(
        ok(&subak, &ep, &["ls"])["terminals"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "loading the state started nothing");
    ok(&subak, &ep, &["host", "stop", "--yes"]);

    // A torn file: reported, then replaced from the start backup by the next Host.
    let file = dir.join("state.json");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, &text[..text.len() / 2]).unwrap();
    let (code, doc) = json(&subak, &ep, &["state", "check", "--state-dir", &d]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (4, json!("state_corrupt"))
    );
    ok(
        &subak,
        &ep,
        &["host", "start", "--no-shell", "--state-dir", &d],
    );
    ok(&subak, &ep, &["host", "stop", "--yes"]);
    let show = ok(&subak, &ep, &["state", "check", "--state-dir", &d]);
    assert_eq!(
        show["terminals"].as_array().unwrap().len(),
        2,
        "recovered: {show}"
    );
}

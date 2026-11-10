//! E2E — Step 26: the tmux compatibility journey on the release binaries (ADR 0060).
//! `SUBAK-FR-060`, `SUBAK-FR-002`, `SUBAK-FR-013`, `SUBAK-FR-018`.
//!
//! A long-running cmd.exe program lives through nested splits, resizes, zoom, every layout
//! preset, break/join, renames, synchronized input, a hook, a watching `subak attach
//! --read-only` client and `subak detach`; its pid never changes and it keeps running with no
//! client. Windows only; a private endpoint; everything started is stopped.
#![cfg(windows)]

use serde_json::Value;
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
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = json(
            self.exe,
            self.ep,
            &["host", "stop", "--kill-terminals", "--yes"],
        );
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_long_running_program_survives_the_tmux_journey() {
    let subak = kim_e2e::bin("subak");
    let ep = format!("subak-e2e26-{}", std::process::id());
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
    };
    let tmux = |line: &str| ok(&subak, &ep, &["command", "--target", "work", line]);
    let panes = || -> Vec<Value> {
        ok(&subak, &ep, &["pane", "ls", "work"])["windows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|w| w["panes"].as_array().unwrap().clone())
            .collect()
    };
    let capture = |id: &str| ok(&subak, &ep, &["capture", id])["lines"].to_string();
    ok(
        &subak,
        &ep,
        &[
            "new",
            "work",
            "--size",
            "160x48",
            "--command-line",
            "cmd.exe /d /q /k \"for /l %i in (1,1,600) do @(echo tick-%i & ping -n 2 127.0.0.1 >nul)\"",
        ],
    );
    let long = panes()[0]["id"].as_str().unwrap().to_string();
    let pid = panes()[0]["pid"].clone();
    wait_for("ticks", || capture(&long).contains("tick-2"));

    tmux(
        "split-window -h; split-window -v; split-window -h -l 30%; select-pane -t work:0.0; split-window -v",
    );
    tmux(
        "resize-pane -t work:0.0 -R 5; resize-pane -Z -t work:0.0; resize-pane -Z -t work:0.0; swap-pane -s work:0.0 -t work:0.2",
    );
    for l in [
        "main-vertical",
        "main-horizontal-mirrored",
        "tiled",
        "even-vertical",
    ] {
        tmux(&format!("select-layout {l}"));
    }
    tmux(&format!("break-pane -s {long} -n ticker"));
    tmux(&format!("join-pane -v -s {long} -t work:0.0"));
    tmux("rename-session -t work w2; rename-session -t w2 work");
    tmux(
        "set-hook after-split-window 'select-layout tiled'; split-window -h; set-hook -u after-split-window",
    );
    let other = panes()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .find(|id| *id != long)
        .unwrap();
    tmux("setw synchronize-panes on");
    ok(
        &subak,
        &ep,
        &[
            "command",
            "--target",
            &other,
            "send-keys 'echo e2e-26-sync' Enter",
        ],
    );
    for p in panes() {
        let id = p["id"].as_str().unwrap().to_string();
        if id != long {
            wait_for("synchronized input", || {
                capture(&id).contains("e2e-26-sync")
            });
        }
    }
    tmux("setw synchronize-panes off");

    // A watching client, then every client detached: nothing ends.
    let mut viewer = Command::new(&subak)
        .args(["--endpoint", &ep, "attach", &long, "--read-only", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .expect("attach");
    wait_for("the viewer attached", || {
        ok(&subak, &ep, &["info", "work"])["terminal"]["clients"].as_u64() == Some(1)
    });
    ok(&subak, &ep, &["detach", "work"]);
    wait_for("the viewer left", || {
        viewer.try_wait().ok().flatten().is_some()
    });
    let _ = viewer.kill();

    let before = capture(&long);
    wait_for("still ticking", || capture(&long) != before);
    let p = panes()
        .into_iter()
        .find(|p| p["id"] == long.as_str())
        .unwrap();
    assert_eq!(p["pid"], pid, "the same process through the whole journey");
    assert_eq!(
        ok(&subak, &ep, &["info", "work"])["terminal"]["state"],
        "running"
    );
}

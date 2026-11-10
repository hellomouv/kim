//! E2E — Step 25: advanced tmux behaviours (ADR 0059). `SUBAK-FR-013`, `SUBAK-FR-017`,
//! `SUBAK-FR-018`.
//!
//! A script drives the release `subak` the way an agent would: a main-vertical layout and the
//! layout cycle, synchronized input reaching every cmd.exe pane, last window and pane through
//! target tokens, `display-message -p` formats, an activity alert in a hidden window, and a hook
//! that re-arranges every new split (refusing `kill-*`).
//!
//! Windows only; a private endpoint; everything started is stopped.
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

fn tmux(exe: &Path, ep: &str, line: &str) -> Vec<Value> {
    ok(exe, ep, &["command", "--target", "work", line])["results"]
        .as_array()
        .unwrap()
        .clone()
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
fn a_script_uses_layouts_sync_tokens_alerts_and_hooks() {
    let subak = kim_e2e::bin("subak");
    let ep = format!("subak-e2e25-{}", std::process::id());
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
    };
    ok(&subak, &ep, &["new", "work", "--size", "200x50"]);
    let windows = |exe: &Path| ok(exe, &ep, &["window", "ls", "work"]);
    let panes = |exe: &Path| -> Vec<Value> {
        windows(exe)["windows"][0]["panes"]
            .as_array()
            .unwrap()
            .clone()
    };
    tmux(&subak, &ep, "split-window -h; split-window -v");
    let first = panes(&subak)[0]["id"].as_str().unwrap().to_string();

    // Layouts: main-vertical puts the first pane 80 columns wide on the left.
    tmux(&subak, &ep, "select-layout main-vertical");
    let p = panes(&subak);
    let main = p.iter().find(|x| x["id"] == first.as_str()).unwrap();
    assert_eq!(
        (main["rect"]["x"].clone(), main["size"]["cols"].clone()),
        (0.into(), 80.into())
    );
    tmux(&subak, &ep, "next-layout");
    let p = panes(&subak);
    assert_ne!(
        p.iter().find(|x| x["id"] == first.as_str()).unwrap()["rect"]["x"],
        0,
        "mirrored"
    );

    // Synchronized input reaches all three shells.
    tmux(&subak, &ep, "setw synchronize-panes on");
    ok(
        &subak,
        &ep,
        &[
            "command",
            "--target",
            &first,
            "send-keys 'echo e2e-sync-25' Enter",
        ],
    );
    for p in panes(&subak) {
        let id = p["id"].as_str().unwrap().to_string();
        wait_for(&format!("sync in {id}"), || {
            ok(&subak, &ep, &["capture", &id])["lines"]
                .to_string()
                .contains("e2e-sync-25")
        });
    }
    tmux(&subak, &ep, "setw synchronize-panes off");

    // Last window and pane through tokens; formats.
    tmux(&subak, &ep, "new-window -n second; last-window");
    let w = windows(&subak);
    assert_eq!(w["active_window"], w["windows"][0]["id"]);
    tmux(&subak, &ep, "select-pane -t work:0.{top-left}");
    let top_left = windows(&subak)["windows"][0]["active_pane"].clone();
    tmux(&subak, &ep, "select-pane -t 'work:.{right-of}'");
    assert_ne!(windows(&subak)["windows"][0]["active_pane"], top_left);
    tmux(&subak, &ep, "last-pane");
    assert_eq!(windows(&subak)["windows"][0]["active_pane"], top_left);
    let r = tmux(&subak, &ep, "display-message -p '#S:#I/#{window_panes}'");
    assert_eq!(r[0]["result"]["message"], "work:0/3");

    // An activity alert in the hidden window; showing it clears the flag.
    tmux(&subak, &ep, "setw -t work:1 monitor-activity on");
    ok(
        &subak,
        &ep,
        &[
            "command",
            "--target",
            "work:1",
            "send-keys 'echo busy' Enter",
        ],
    );
    wait_for("the activity alert", || {
        windows(&subak)["windows"][1]["activity"] == true
    });
    tmux(&subak, &ep, "select-window -t work:1; last-window");
    assert_eq!(windows(&subak)["windows"][1]["activity"], false);

    // A hook re-arranges every new split; kill-* is refused in a hook.
    tmux(
        &subak,
        &ep,
        "set-hook after-split-window 'select-layout even-vertical'",
    );
    tmux(&subak, &ep, "split-window -h");
    wait_for("the hook's layout", || {
        let p = panes(&subak);
        p.len() == 4 && p.iter().all(|x| x["rect"]["x"] == 0)
    });
    let (code, _) = json(
        &subak,
        &ep,
        &["command", "set-hook pane-exited 'kill-session -t work'"],
    );
    assert_ne!(code, 0);
}

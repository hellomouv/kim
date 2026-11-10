//! Step 25 — advanced tmux behaviours on real Windows (ADR 0059, docs/SUBAK_CLI.md §3.17):
//! layout presets, `synchronize-panes`, last window/pane and target tokens, activity and bell
//! alerts, `display-message` formats, hooks. `SUBAK-FR-013`, `SUBAK-FR-017`, `SUBAK-FR-018`.
//!
//! Real `subak.exe` Hosts on their own endpoints; every `--json` document is validated against
//! `subak-cli-v1`, IPC results inside against their `ipc-v1` definitions. Each test stops its
//! Host with `--kill-terminals --yes`. Nothing opens a window.
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::Value;
use std::time::{Duration, Instant};

struct Session {
    ep: String,
    host: Proc,
}

impl Session {
    fn start(tag: &str) -> Self {
        let ep = endpoint(tag);
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
        Self {
            host: Proc::open(pid_of(&doc["data"]["host"]["pid"])),
            ep,
        }
    }

    fn cli(&self, args: &[&str]) -> (i32, Value) {
        let mut a = vec!["--endpoint", self.ep.as_str()];
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

    /// One tmux command line on `target`; the results.
    fn tmux(&self, target: &str, line: &str) -> Vec<Value> {
        self.ok(&["command", "--target", target, line])["results"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn fails(&self, args: &[&str]) -> Value {
        let (exit, doc) = self.cli(args);
        assert_ne!(exit, 0, "{args:?} must fail: {doc}");
        doc["error"].clone()
    }

    fn windows(&self, t: &str) -> Vec<Value> {
        self.ok(&["window", "ls", t])["windows"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn panes(&self, t: &str) -> Vec<Value> {
        self.windows(t)
            .iter()
            .flat_map(|w| w["panes"].as_array().unwrap().clone())
            .collect()
    }

    fn until(&self, what: &str, mut f: impl FnMut() -> bool) {
        let end = Instant::now() + LONG;
        while !f() {
            assert!(Instant::now() < end, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn captured(&self, target: &str, needle: &str) -> bool {
        self.ok(&["capture", target])["lines"]
            .to_string()
            .contains(needle)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.host.alive() {
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
        }
    }
}

/// Layout presets (main-*, mirrored, next/previous, spread), `synchronize-panes`, last window
/// and pane with target tokens, and `display-message` formats, on real cmd.exe panes.
#[test]
fn layouts_sync_tokens_and_formats_act_on_real_panes() {
    let s = Session::start("w25-adv");
    s.ok(&["new", "work", "--size", "200x50"]);
    s.tmux("work", "split-window -h; split-window -v");
    let first = s.panes("work")[0]["id"].as_str().unwrap().to_string();

    // main-vertical: the first pane is main-pane-width (80) wide on the left.
    let r = s.tmux("work", "select-layout main-vertical");
    assert_eq!(r[0]["method"], "window.select_layout");
    let p = s.panes("work");
    let main = p.iter().find(|x| x["id"] == first.as_str()).unwrap();
    assert_eq!(main["rect"]["x"], 0);
    assert_eq!(main["size"]["cols"], 80, "{main}");
    // next-layout follows tmux's cycle: main-vertical-mirrored puts it on the right.
    s.tmux("work", "next-layout");
    let p = s.panes("work");
    let main = p.iter().find(|x| x["id"] == first.as_str()).unwrap();
    assert_eq!(main["rect"]["x"], 120, "{main}");
    // main-horizontal: 24 rows on top, the others side by side below.
    s.tmux("work", "selectl main-horizontal");
    let p = s.panes("work");
    let main = p.iter().find(|x| x["id"] == first.as_str()).unwrap();
    assert_eq!(
        (main["rect"]["y"].clone(), main["size"]["rows"].clone()),
        (0.into(), 24.into())
    );
    assert!(
        p.iter()
            .filter(|x| x["id"] != first.as_str())
            .all(|x| x["rect"]["y"] == 25)
    );
    // -E spreads every split equally; previous-layout steps back.
    s.tmux("work", "selectl -E; previous-layout");
    s.fails(&["command", "--target", "work", "select-layout spiral"]);

    // synchronize-panes: one pane's input reaches every pane of the window.
    let r = s.tmux("work", "setw synchronize-panes on");
    ipc_def("result_window_options", &r[0]["result"]);
    assert_eq!(r[0]["result"]["options"]["synchronize-panes"], true);
    s.tmux(&first, "send-keys 'echo sync-hit-1' Enter");
    for p in s.panes("work") {
        let id = p["id"].as_str().unwrap().to_string();
        s.until(&format!("the synchronized input in {id}"), || {
            s.captured(&id, "sync-hit-1")
        });
    }
    let r = s.tmux("work", "show-options");
    ipc_def("result_window_options", &r[0]["result"]);
    assert_eq!(s.windows("work")[0]["options"]["synchronize-panes"], true);
    s.tmux("work", "setw synchronize-panes off");
    s.tmux(&first, "send-keys 'echo only-first-2' Enter");
    s.until("the first pane's own input", || {
        s.captured(&first, "only-first-2")
    });
    std::thread::sleep(Duration::from_millis(500));
    let other = s.panes("work")[1]["id"].as_str().unwrap().to_string();
    assert!(!s.captured(&other, "only-first-2"), "off: no fan-out");

    // last-pane and the pane tokens.
    let before = s.ok(&["window", "ls", "work"])["windows"][0]["active_pane"].clone();
    s.tmux("work", "select-pane -t work:0.{top-left}");
    assert_eq!(s.windows("work")[0]["active_pane"], first.as_str());
    s.tmux("work", "last-pane");
    assert_eq!(
        s.windows("work")[0]["active_pane"],
        before,
        "back to the previous pane"
    );
    // Tiled (the previous layout of a spread one): the first pane is top left.
    s.tmux("work", "select-pane -t work:0.{top-left}");
    s.tmux("work", "select-pane -t 'work:.{right-of}'");
    assert_ne!(s.windows("work")[0]["active_pane"], first.as_str());
    s.tmux("work", "select-pane -t 'work:.{left-of}'");
    assert_eq!(s.windows("work")[0]["active_pane"], first.as_str());

    // last-window and the window tokens.
    let e = s.fails(&["command", "--target", "work", "last-window"]);
    assert_eq!(e["code"], "target_not_found", "no previous window yet: {e}");
    s.tmux("work", "new-window -n second");
    let t = s.ok(&["window", "ls", "work"]);
    assert_eq!(t["active_window"], s.windows("work")[1]["id"]);
    s.tmux("work", "last-window");
    let t = s.ok(&["window", "ls", "work"]);
    assert_eq!(t["active_window"], t["windows"][0]["id"]);
    assert_eq!(t["last_window"], t["windows"][1]["id"]);
    s.tmux("work", "select-window -t 'work:{last}'");
    assert_eq!(
        s.ok(&["window", "ls", "work"])["active_window"],
        t["windows"][1]["id"]
    );
    s.tmux("work", "select-window -t work:+1");
    assert_eq!(
        s.ok(&["window", "ls", "work"])["active_window"],
        t["windows"][0]["id"],
        "wraps"
    );

    // display-message -p: tmux formats of the target.
    let r = s.tmux(
        "work",
        "display-message -p '#S:#I.#P #{window_panes} #{?pane_synchronized,sync,nosync} ##'",
    );
    assert_eq!(r[0]["method"], "format");
    let msg = r[0]["result"]["message"].as_str().unwrap();
    assert!(msg.starts_with("work:0."), "{msg}");
    assert!(msg.ends_with(" 3 nosync #"), "{msg}");
    let r = s.tmux("work", "display -p '#{pane_current_command}'");
    assert_eq!(r[0]["result"]["message"], "cmd");
    let r = s.tmux("work", "list-commands");
    assert!(
        r[0]["result"]["text"]
            .as_str()
            .unwrap()
            .contains("set-window-option (setw")
    );

    // Errors are stable and never guess.
    s.fails(&["command", "--target", "work", "setw -g monitor-bell off"]);
    s.fails(&["command", "--target", "work", "setw status on"]);
    s.fails(&["command", "select-window -t {last}"]);
}

/// Activity and bell alerts in windows that are not shown, cleared by showing the window, and
/// hooks that run tmux commands after the Host's events.
#[test]
fn alerts_and_hooks_follow_host_events() {
    let s = Session::start("w25-hooks");
    s.ok(&["new", "work", "--size", "120x40"]);
    s.tmux("work", "new-window -d -n quiet");
    let ws = s.windows("work");
    assert_eq!(ws[1]["options"]["monitor-activity"], false);
    assert_eq!(ws[1]["options"]["monitor-bell"], true, "tmux's default");

    // monitor-activity: output in the hidden window flags it once.
    s.tmux("work", "setw -t work:1 monitor-activity on");
    s.tmux("work:1", "send-keys 'echo busy' Enter");
    s.until("the activity flag", || {
        s.windows("work")[1]["activity"] == true
    });
    assert_eq!(
        s.windows("work")[0]["activity"],
        false,
        "the shown window never flags"
    );
    // Showing the window clears it.
    s.tmux("work", "select-window -t work:1; select-window -t work:0");
    assert_eq!(s.windows("work")[1]["activity"], false);

    // monitor-bell: a BEL from the program flags the window (cmd.exe echoes ^G as BEL).
    s.tmux("work", "setw -t work:1 monitor-activity off");
    s.tmux("work:1", "send-keys 'echo ' C-g Enter");
    s.until("the bell flag", || s.windows("work")[1]["bell"] == true);
    let r = s.tmux("work", "display -p -t work:1 '#{window_flags}'");
    assert_eq!(
        r[0]["result"]["message"], "-!",
        "the last window, with a bell"
    );

    // Hooks: after-split-window re-arranges every new split; its own events never loop.
    let r = s.tmux(
        "work",
        "set-hook after-split-window 'select-layout even-vertical'",
    );
    ipc_def("result_hooks", &r[0]["result"]);
    s.tmux("work", "split-window -h");
    s.until("the hook's layout", || {
        let p = s.windows("work")[0]["panes"].as_array().unwrap().clone();
        p.len() == 2 && p.iter().all(|x| x["rect"]["x"] == 0)
    });
    let r = s.tmux("work", "show-hooks");
    ipc_def("result_hooks", &r[0]["result"]);
    assert_eq!(
        r[0]["result"]["hooks"]["after-split-window"],
        "select-layout even-vertical"
    );
    assert!(
        r[0]["result"]["available"]
            .to_string()
            .contains("alert-bell")
    );
    let r = s.tmux("work", "set-hook -u after-split-window");
    assert!(r[0]["result"]["hooks"].as_object().unwrap().is_empty());
    s.tmux("work", "split-window -h");
    std::thread::sleep(Duration::from_millis(500));
    let p = s.windows("work")[0]["panes"].as_array().unwrap().clone();
    assert!(
        p.iter().any(|x| x["rect"]["x"] != 0),
        "no hook: a side-by-side split"
    );
    // Unknown hooks and command lines that do not parse are refused.
    s.fails(&["command", "set-hook client-attached 'lsp'"]);
    s.fails(&["command", "set-hook pane-exited 'nope-command'"]);
    s.fails(&["command", "set-hook pane-exited 'kill-window -t work:1'"]);
}

//! Step 24 — tmux command lines (`subak command`) and paste buffers (`subak buffer`) on real
//! Windows (ADR 0058, docs/SUBAK_CLI.md §3.16). `SUBAK-FR-013`, `SUBAK-FR-017`, `SUBAK-FR-018`.
//!
//! Real `subak.exe` Hosts on their own endpoints; every `--json` document is validated against
//! `subak-cli-v1`, every IPC result inside against its `ipc-v1` definition. Each test stops its
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

    fn cli_env(&self, args: &[&str], env: &[(&str, &str)]) -> (i32, Value) {
        let mut a = vec!["--endpoint", self.ep.as_str()];
        a.extend_from_slice(args);
        let (code, doc) = run_env(&a, env);
        validate("subak-cli-v1", &doc);
        (code, doc)
    }

    fn cli(&self, args: &[&str]) -> (i32, Value) {
        self.cli_env(args, &[])
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn fails(&self, args: &[&str], code: &str) -> Value {
        let (exit, doc) = self.cli(args);
        assert_ne!(exit, 0, "{args:?} must fail: {doc}");
        assert_eq!(doc["error"]["code"], code, "{args:?}: {doc}");
        doc["error"].clone()
    }

    fn panes(&self, t: &str) -> Vec<Value> {
        let d = self.ok(&["pane", "ls", t]);
        d["windows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|w| w["panes"].as_array().unwrap().clone())
            .collect()
    }

    fn windows(&self, t: &str) -> Vec<Value> {
        self.ok(&["window", "ls", t])["windows"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn captured(&self, target: &str, needle: &str) {
        let until = Instant::now() + LONG;
        loop {
            let d = self.ok(&["capture", target]);
            if d["lines"].to_string().contains(needle) {
                return;
            }
            assert!(Instant::now() < until, "`{needle}` not in {target}: {d}");
            std::thread::sleep(Duration::from_millis(100));
        }
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

fn results(d: &Value) -> Vec<Value> {
    d["results"].as_array().unwrap().clone()
}

/// tmux command lines: chains, flags, quoting, targets, the context of a pane, moving panes
/// between windows, send-keys, consent for kills, dry runs and stable errors.
#[test]
fn command_lines_split_move_send_and_list_with_tmux_semantics() {
    let s = Session::start("w24-cmd");
    s.ok(&["new", "work", "--size", "100x30"]);
    let d = s.ok(&[
        "command",
        "--target",
        "work",
        "split-window -h; split-window -v -l 30%; list-panes",
    ]);
    let r = results(&d);
    assert_eq!(r.len(), 3);
    ipc_def("result_window_create", &r[0]["result"]);
    ipc_def("result_topology", &r[2]["result"]);
    assert_eq!(r[1]["method"], "pane.split");
    assert_eq!(s.panes("work").len(), 3);
    let second = r[0]["result"]["pane_id"].as_str().unwrap().to_string();

    // break-pane: the pane gets a window of its own; its process is the same.
    let pid = s
        .panes("work")
        .iter()
        .find(|p| p["id"] == second.as_str())
        .unwrap()["pid"]
        .clone();
    let d = s.ok(&["command", "--target", &second, "break-pane -n 'moved out'"]);
    ipc_def("result_move_pane", &results(&d)[0]["result"]);
    let ws = s.windows("work");
    assert_eq!(ws.len(), 2);
    assert_eq!(ws[1]["name"], "moved out");
    // join-pane: back as a side-by-side split of pane 0.0; the moved-out window goes.
    let d = s.ok(&[
        "command",
        "--",
        "join-pane",
        "-h",
        "-s",
        &second,
        "-t",
        "work:0.0",
    ]);
    let res = &results(&d)[0]["result"];
    ipc_def("result_move_pane", res);
    assert_eq!(res["window_removed"], true);
    assert_eq!(s.windows("work").len(), 1);
    let joined = s.panes("work");
    assert_eq!(joined.len(), 3);
    assert_eq!(
        joined.iter().find(|p| p["id"] == second.as_str()).unwrap()["pid"],
        pid,
        "the same process moved twice"
    );

    // send-keys: tmux key names and literal text reach the program.
    let d = s.ok(&[
        "command",
        "--target",
        "work:0.0",
        "send-keys 'echo sent-by-tm^ux' Enter",
    ]);
    ipc_def("result_input", &results(&d)[0]["result"]);
    s.captured("work:0.0", "sent-by-tmux");

    // The context of a pane: inside a Subak pane, a command without -t acts on that pane.
    let t = s.ok(&["info", "work"])["terminal"].clone();
    let tid = t["id"].as_str().unwrap().to_string();
    let first = s.panes("work")[0]["id"].as_str().unwrap().to_string();
    let (code, doc) = s.cli_env(
        &["command", "select-pane -R"],
        &[("SUBAK_PANE_ID", &first), ("SUBAK_TERMINAL_ID", &tid)],
    );
    assert_eq!(code, 0, "{doc}");
    assert_ne!(
        doc["data"]["results"][0]["result"]["pane_id"],
        first.as_str(),
        "the pane to the right of the caller's pane"
    );
    let (code, doc) = s.cli_env(
        &["command", "new-window -d -n fromctx"],
        &[("SUBAK_PANE_ID", &first), ("SUBAK_TERMINAL_ID", &tid)],
    );
    assert_eq!(code, 0, "{doc}");
    assert_eq!(s.windows("work").len(), 2);

    // A dry run plans and changes nothing.
    s.fails(
        &[
            "command",
            "--target",
            "work",
            "--dry-run",
            "splitw -h; killp; lsb",
        ],
        "confirmation_required",
    );
    let before = s.panes("work").len();
    let d = s.ok(&["command", "--target", "work", "--dry-run", "splitw -h; lsb"]);
    assert_eq!(d["would_run"][0]["method"], "pane.split");
    assert_eq!(s.panes("work").len(), before);

    // Stable errors, never guesses.
    let e = s.fails(&["command", "--target", "work", "splt-window"], "usage");
    assert!(e["details"].to_string().contains("unknown_command"), "{e}");
    let e = s.fails(&["command", "select-pane -t {last}"], "usage");
    assert!(e["details"].to_string().contains("bad_target"), "{e}");
    s.fails(&["command", "--target", "work", "split-window -f"], "usage");
    s.fails(&["command", "--target", "work", "copy-mode"], "usage");
    s.fails(&["command", "split-window"], "usage"); // no context, no -t
    s.fails(
        &["command", "--target", "work", "kill-pane"],
        "confirmation_required",
    );
    // A chain stops at the first refusal; what ran before it stays.
    let n = s.panes("work").len();
    s.fails(
        &[
            "command",
            "--target",
            "work",
            "split-window -h; select-window -t work:7; split-window -h",
        ],
        "target_not_found",
    );
    assert_eq!(s.panes("work").len(), n + 1);
    // With consent, kill-pane ends that pane only.
    let n = s.panes("work").len();
    s.ok(&["command", "--target", "work:0.0", "--yes", "kill-pane"]);
    assert_eq!(s.panes("work").len(), n - 1);
}

/// Paste buffers: kept by the Host in memory, newest first, previews redacted, pasted as input,
/// at most 50 (the oldest automatic one goes; named ones stay), dry runs change nothing.
#[test]
fn paste_buffers_live_in_the_host_and_paste_as_input() {
    let s = Session::start("w24-buf");
    s.ok(&["new", "work"]);
    let d = s.ok(&["buffer", "set", "echo from-buff^er\n"]);
    assert_eq!(d["name"], "buffer0");
    s.ok(&["buffer", "set", "--name", "notes", "line1\nline2"]);
    s.ok(&[
        "buffer",
        "set",
        "token ghp_abcdefghijklmnopqrstuvwxyz0123456789AB",
    ]);
    let d = s.ok(&["buffer", "ls"]);
    let list = d["buffers"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["name"], "buffer1", "newest first");
    assert!(
        !list[0]["preview"].as_str().unwrap().contains("ghp_abc"),
        "{list:?}"
    );
    assert_eq!(list[1]["lines"], 2);
    assert_eq!(s.ok(&["buffer", "show", "notes"])["data"], "line1\nline2");
    let d = s.ok(&["buffer", "paste", "work", "--name", "buffer0"]);
    assert_eq!(d["name"], "buffer0");
    s.captured("work", "from-buffer");
    // tmux commands reach the same buffers.
    let d = s.ok(&[
        "command",
        "--target",
        "work",
        "set-buffer -b x 'echo via-comma^nd'",
    ]);
    assert_eq!(results(&d)[0]["result"]["name"], "x");
    s.ok(&[
        "command",
        "--target",
        "work",
        "paste-buffer -b x -d; send-keys Enter",
    ]);
    s.captured("work", "via-command");
    s.fails(&["buffer", "show", "x"], "target_not_found");
    // Dry runs.
    s.ok(&["buffer", "set", "--dry-run", "nothing"]);
    s.ok(&["buffer", "delete", "--dry-run", "notes"]);
    assert_eq!(
        s.ok(&["buffer", "ls"])["buffers"].as_array().unwrap().len(),
        3
    );
    // The limit: 50; the oldest automatic buffer goes first, a named one stays.
    for i in 0..50 {
        s.ok(&["buffer", "set", &format!("filler {i}")]);
    }
    let d = s.ok(&["buffer", "ls"]);
    let names: Vec<&str> = d["buffers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 50);
    assert!(names.contains(&"notes"), "a named buffer stays");
    assert!(
        !names.contains(&"buffer0"),
        "the oldest automatic buffer went"
    );
    s.ok(&["buffer", "delete", "notes"]);
    s.fails(&["buffer", "show", "notes"], "target_not_found");
}

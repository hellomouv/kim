//! Step 26 — the tmux compatibility journeys on real Windows (docs/TMUX_COMPATIBILITY.md §0,
//! "How evidence is recorded"). `SUBAK-FR-002`, `SUBAK-FR-013`, `SUBAK-FR-017`, `SUBAK-FR-018`.
//!
//! One long-running program (a cmd.exe that keeps printing) lives through everything a tmux user
//! does to its Terminal: nested splits, resizes, zoom, swaps, layouts, break and join, window
//! moves, renames, synchronized input, hooks, two clients watching at once, detaching every
//! client and the CLI leaving. Its pid never changes, it answers at the end, and the Terminal
//! keeps running with no client at all. Real `subak.exe` Host, private endpoint, nothing on
//! screen; JSON documents are validated against `subak-cli-v1`.
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn cli(ep: &str, args: &[&str]) -> Value {
    let mut a = vec!["--endpoint", ep];
    a.extend_from_slice(args);
    let (code, doc) = run_env(&a, &[]);
    validate("subak-cli-v1", &doc);
    assert_eq!(code, 0, "{args:?}: {doc}");
    doc["data"].clone()
}

fn tmux(ep: &str, line: &str) -> Value {
    cli(ep, &["command", "--target", "work", line])
}

fn all_panes(ep: &str) -> Vec<Value> {
    cli(ep, &["pane", "ls", "work"])["windows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|w| w["panes"].as_array().unwrap().clone())
        .collect()
}

fn pid_of_pane(ep: &str, id: &str) -> Value {
    all_panes(ep)
        .into_iter()
        .find(|p| p["id"] == id)
        .unwrap_or_else(|| panic!("pane {id} is gone"))["pid"]
        .clone()
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + LONG;
    while !f() {
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn shows(ep: &str, pane: &str, needle: &str) -> bool {
    cli(ep, &["capture", pane])["lines"]
        .to_string()
        .contains(needle)
}

/// A read-only viewer: waits until `needle` arrives on its stream.
fn sees(c: &Raw, needle: &str) {
    let end = Instant::now() + LONG;
    let mut seen = Vec::new();
    while !String::from_utf8_lossy(&seen).contains(needle) {
        assert!(Instant::now() < end, "a viewer never saw `{needle}`");
        if let Got::Data(b) = c.next(Duration::from_millis(200)) {
            seen.extend(b);
        }
    }
}

#[test]
fn a_long_running_program_survives_every_multiplexer_journey() {
    let h = Host::start("w26-journey", &["--no-shell", "--idle-exit", "120"]);
    let ep = h.ep.clone();
    // The long-running program: a cmd.exe printing a tick every second, for minutes.
    cli(
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
    let long = all_panes(&ep)[0]["id"].as_str().unwrap().to_string();
    let pid = pid_of_pane(&ep, &long);
    until("the program ticks", || shows(&ep, &long, "tick-2"));

    // Nested splits, resizes, zoom, swaps and every layout preset.
    tmux(
        &ep,
        "split-window -h; split-window -v; split-window -h -l 30%; select-pane -t work:0.0; split-window -v",
    );
    assert_eq!(all_panes(&ep).len(), 5);
    tmux(
        &ep,
        "resize-pane -t work:0.0 -R 7; resize-pane -t work:0.0 -D 3; resize-pane -Z -t work:0.0; resize-pane -Z -t work:0.0",
    );
    tmux(&ep, "swap-pane -s work:0.0 -t work:0.3; rotate-window");
    for l in [
        "even-horizontal",
        "even-vertical",
        "main-horizontal",
        "main-horizontal-mirrored",
        "main-vertical",
        "main-vertical-mirrored",
        "tiled",
    ] {
        tmux(&ep, &format!("select-layout {l}"));
    }
    tmux(&ep, "next-layout; previous-layout; select-layout -E");
    assert_eq!(pid_of_pane(&ep, &long), pid);
    // Every program sees its own size (the layout is real, not metadata).
    for p in all_panes(&ep) {
        assert!(p["size"]["cols"].as_u64().unwrap() >= 2, "{p}");
    }

    // Windows: break the program out, rename, move it back, select and last-window.
    tmux(&ep, &format!("break-pane -s {long} -n longrun"));
    tmux(
        &ep,
        "rename-window -t work:1 ticker; select-window -t work:0; last-window",
    );
    assert_eq!(
        cli(&ep, &["window", "ls", "work"])["windows"][1]["name"],
        "ticker"
    );
    tmux(&ep, &format!("join-pane -h -s {long} -t work:0.0"));
    assert_eq!(
        cli(&ep, &["window", "ls", "work"])["windows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        pid_of_pane(&ep, &long),
        pid,
        "moved twice, the same process"
    );
    tmux(
        &ep,
        "rename-session -t work work2; rename-session -t work2 work",
    );

    // Synchronized input and a hook, then off again.
    tmux(
        &ep,
        "set-hook after-split-window 'select-layout tiled'; split-window -v; set-hook -u after-split-window",
    );
    let others: Vec<String> = all_panes(&ep)
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .filter(|id| *id != long)
        .collect();
    tmux(&ep, "setw synchronize-panes on");
    cli(
        &ep,
        &[
            "command",
            "--target",
            &others[0],
            "send-keys 'echo journey-sync' Enter",
        ],
    );
    for id in &others {
        until(&format!("synchronized input in {id}"), || {
            shows(&ep, id, "journey-sync")
        });
    }
    tmux(&ep, "setw synchronize-panes off");

    // Two clients watch the program at once; both see the same live output.
    let a = Raw::handshake(&ep, "cli");
    let b = Raw::handshake(&ep, "gui");
    for c in [&a, &b] {
        let r = c.request(
            2,
            "pane.attach",
            &json!({"target": long, "interactive": false}),
        );
        assert_eq!(r["ok"], true, "{r}");
    }
    sees(&a, "tick-");
    sees(&b, "tick-");
    // Detaching every client of the Terminal, then the clients going away, ends nothing.
    cli(&ep, &["detach", "work"]);
    drop(a);
    drop(b);
    h.wait_connected(1);

    // The program is the same process, still ticking, and the Terminal runs with no client.
    let before = cli(&ep, &["capture", &long])["lines"].clone();
    until("the program keeps ticking", || {
        cli(&ep, &["capture", &long])["lines"] != before
    });
    assert_eq!(pid_of_pane(&ep, &long), pid);
    let t = cli(&ep, &["info", "work"])["terminal"].clone();
    assert_eq!(t["state"], "running", "{t}");
    assert!(
        Proc::try_open(u32::try_from(pid.as_u64().unwrap()).unwrap()).is_some_and(|p| p.alive())
    );
}

//! Step 22 — pane splits and the layout engine on real Windows (ADR 0056,
//! docs/SESSION_MODEL.md §3.5, docs/contracts/IPC_PROTOCOL.md §4.4). `SUBAK-FR-013`,
//! `SUBAK-FR-014`, `SUBAK-FR-017`, `SUBAK-FR-004`.
//!
//! Real `subak.exe` Hosts, each on its **own** endpoint, driven by the `subak` CLI (every
//! `--json` document validated against `subak-cli-v1`, results against their `ipc-v1`
//! definition) and raw IPC v1 clients (every event validated). The pseudo console sizes are
//! checked from inside the panes: a PowerShell loop prints `[Console]::WindowWidth` and
//! `WindowHeight`, which is what ConPTY gives its programs. Each test stops its Host with
//! `--kill-terminals --yes` and checks that every pane process is gone. Nothing opens a window.
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// A program that prints its console size four times a second: `SZ <cols>x<rows>`.
const REPORTER: &[&str] = &[
    "--",
    "powershell.exe",
    "-NoLogo",
    "-NoProfile",
    "-Command",
    "while($true){'SZ '+[Console]::WindowWidth+'x'+[Console]::WindowHeight; Start-Sleep -Milliseconds 250}",
];

struct Session {
    ep: String,
    host: Proc,
    seen: std::sync::Mutex<Vec<u32>>,
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
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn cli(&self, args: &[&str]) -> (i32, Value) {
        let mut a = vec!["--endpoint", self.ep.as_str()];
        a.extend_from_slice(args);
        let (code, doc) = run(&a);
        validate("subak-cli-v1", &doc);
        (code, doc)
    }

    fn ok(&self, args: &[&str], def: Option<&str>) -> Value {
        let (code, doc) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        let mut d = doc["data"].clone();
        if let Some(o) = d.as_object_mut() {
            o.remove("host");
        }
        if let Some(def) = def {
            ipc_def(def, &d);
        }
        remember(&self.seen, &d);
        d
    }

    fn fails(&self, args: &[&str], code: &str) -> Value {
        let (exit, doc) = self.cli(args);
        assert_ne!(exit, 0, "{args:?} must fail: {doc}");
        assert_eq!(doc["error"]["code"], code, "{args:?}: {doc}");
        doc["error"].clone()
    }

    /// The panes of the Terminal's first window, in order.
    fn panes(&self, target: &str) -> (Value, Vec<Value>) {
        let d = self.ok(&["pane", "ls", target], Some("result_topology"));
        let w = d["windows"][0].clone();
        let ps = w["panes"].as_array().unwrap().clone();
        (w, ps)
    }

    /// Waits until the pane's program reports `SZ <cols>x<rows>` as its newest line.
    fn reports(&self, pane: &str, cols: u64, rows: u64) {
        let want = format!("SZ {cols}x{rows}");
        let until = Instant::now() + LONG;
        let mut last = String::new();
        loop {
            let (_, doc) = self.cli(&["capture", pane]);
            last = doc["data"]["lines"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter_map(Value::as_str)
                        .rfind(|s| s.starts_with("SZ "))
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or(last);
            if last.trim() == want {
                return;
            }
            assert!(
                Instant::now() < until,
                "pane {pane}: wanted `{want}`, it reports `{last}`"
            );
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
        let mut survivors = Vec::new();
        for pid in self.seen.lock().unwrap().iter() {
            if let Some(p) = Proc::try_open(*pid)
                && !p.wait(Duration::from_secs(10))
            {
                p.kill();
                survivors.push(*pid);
            }
        }
        if !std::thread::panicking() {
            assert!(survivors.is_empty(), "outlived the Host: {survivors:?}");
        }
    }
}

fn remember(seen: &std::sync::Mutex<Vec<u32>>, v: &Value) {
    match v {
        Value::Object(m) => {
            if m.contains_key("stream_id")
                && let Some(pid) = m.get("pid").and_then(Value::as_u64)
            {
                seen.lock().unwrap().push(u32::try_from(pid).unwrap());
            }
            m.values().for_each(|c| remember(seen, c));
        }
        Value::Array(a) => a.iter().for_each(|c| remember(seen, c)),
        _ => {}
    }
}

fn alive(pid: u32) -> bool {
    Proc::try_open(pid).is_some_and(|p| p.alive())
}

fn rect(p: &Value) -> (u64, u64, u64, u64) {
    let r = &p["rect"];
    (
        r["x"].as_u64().unwrap(),
        r["y"].as_u64().unwrap(),
        r["cols"].as_u64().unwrap(),
        r["rows"].as_u64().unwrap(),
    )
}

fn id(p: &Value) -> String {
    p["id"].as_str().unwrap().to_string()
}

fn events_until(c: &Raw, mut until: impl FnMut(&[Value]) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + LONG;
    let mut got = Vec::new();
    while !until(&got) {
        assert!(
            Instant::now() < deadline,
            "events so far: {:#}",
            Value::from(got.clone())
        );
        if let Got::Control(m) = c.next_control(Duration::from_millis(200))
            && m["type"] == "event"
        {
            ipc_valid(&m);
            got.push(m);
        }
    }
    got
}

fn layouts(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|e| e["event"] == "window.layout")
        .map(|e| &e["data"])
        .collect()
}

// ---------------------------------------------------------------------------------------------

/// SUBAK-FR-013/014: a Terminal is split side by side and stacked, its panes resized, zoomed,
/// swapped and closed; every program keeps running and sees exactly its share of the window.
#[test]
fn splits_resize_zoom_swap_and_close_keep_processes_and_real_pty_sizes() {
    let s = Session::start("w22-split");
    let mut args = vec!["new", "work", "--size", "81x24"];
    args.extend_from_slice(REPORTER);
    s.ok(&args, None);
    let (_, ps) = s.panes("work");
    let left = id(&ps[0]);
    s.reports(&left, 81, 24);

    // Side by side: two halves with a one-cell border.
    let mut a = vec!["pane", "split", "work", "--horizontal"];
    a.extend_from_slice(REPORTER);
    let d = s.ok(&a, Some("result_window_create"));
    let right = d["pane_id"].as_str().unwrap().to_string();
    assert_eq!(d["selected"], true);
    let (w, ps) = s.panes("work");
    assert_eq!(w["layout"]["type"], "split");
    assert_eq!(w["layout"]["direction"], "horizontal");
    assert_eq!(rect(&ps[0]), (0, 0, 40, 24));
    assert_eq!(rect(&ps[1]), (41, 0, 40, 24));
    s.reports(&left, 40, 24);
    s.reports(&right, 40, 24);

    // Stacked inside the right half: a nested split.
    let mut a = vec!["pane", "split", right.as_str(), "--vertical"];
    a.extend_from_slice(REPORTER);
    let below = s.ok(&a, Some("result_window_create"))["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (w, ps) = s.panes("work");
    assert_eq!(w["layout"]["children"][1]["node"]["direction"], "vertical");
    assert_eq!(
        ps.iter().map(id).collect::<Vec<_>>(),
        [left.clone(), right.clone(), below.clone()]
    );
    // 23 rows to share: the split pane keeps the odd one.
    assert_eq!(rect(&ps[1]), (41, 0, 40, 12));
    assert_eq!(rect(&ps[2]), (41, 13, 40, 11));
    s.reports(&right, 40, 12);
    s.reports(&below, 40, 11);
    let pids: Vec<u32> = ps.iter().map(|p| pid_of(&p["pid"])).collect();

    // Resize: the left pane's border moves 10 cells right; both right panes shrink.
    let d = s.ok(
        &["pane", "resize", left.as_str(), "right", "10"],
        Some("result_layout"),
    );
    assert_eq!(d["moved"], 10);
    s.reports(&left, 50, 24);
    s.reports(&right, 30, 12);
    s.reports(&below, 30, 11);
    // Never below the minimum: a huge move stops at 2 columns.
    let d = s.ok(
        &["pane", "resize", left.as_str(), "right", "500"],
        Some("result_layout"),
    );
    assert_eq!(d["moved"], 28);
    s.ok(
        &["pane", "resize", left.as_str(), "left", "28"],
        Some("result_layout"),
    );

    // Zoom: the pane gets the whole window, the others keep their sizes unseen.
    let d = s.ok(&["pane", "zoom", right.as_str()], Some("result_layout"));
    assert_eq!(d["zoomed"], right.as_str());
    assert_eq!(d["active_pane"], right.as_str(), "a zoomed pane is shown");
    s.reports(&right, 81, 24);
    s.reports(&left, 50, 24);
    let (w, ps) = s.panes("work");
    assert_eq!(w["zoomed"], right.as_str());
    assert_eq!(ps[0]["visible"], false);
    // Selecting another pane unzooms (tmux), and the zoomed pane gets its place back.
    s.ok(&["pane", "select", left.as_str()], Some("result_select"));
    s.reports(&right, 30, 12);
    let (w, _) = s.panes("work");
    assert!(w["zoomed"].is_null());
    s.ok(
        &["pane", "zoom", left.as_str(), "on"],
        Some("result_layout"),
    );
    s.reports(&left, 81, 24);
    let d = s.ok(
        &["pane", "zoom", left.as_str(), "off"],
        Some("result_layout"),
    );
    assert!(d["zoomed"].is_null());
    s.reports(&left, 50, 24);

    // Swap: places change, processes stay with their panes and get the new sizes.
    let d = s.ok(
        &["pane", "swap", left.as_str(), below.as_str()],
        Some("result_layout"),
    );
    assert_eq!(d["changed"], true);
    let (_, ps) = s.panes("work");
    assert_eq!(id(&ps[0]), below);
    assert_eq!(
        pid_of(&ps[0]["pid"]),
        pids[2],
        "the process moved with its pane"
    );
    s.reports(&below, 50, 24);
    s.reports(&left, 30, 11);

    // Close: the freed space goes to the neighbour; nothing else ends.
    s.ok(&["pane", "kill", right.as_str(), "--yes"], None);
    let (w, ps) = s.panes("work");
    assert_eq!(ps.len(), 2);
    assert_eq!(rect(&ps[1]), (51, 0, 30, 24));
    assert!(w["layout"]["children"].as_array().unwrap().len() == 2);
    s.reports(&left, 30, 24);
    assert!(!alive(pids[1]));
    assert!(alive(pids[0]) && alive(pids[2]));
}

/// SUBAK-FR-013/017: directional focus follows the geometry and wraps, a pane too small to
/// split is refused with nothing changed, dry runs change nothing, bad requests are usage errors.
#[test]
fn directional_focus_minimum_sizes_dry_runs_and_refusals() {
    let s = Session::start("w22-focus");
    s.ok(&["new", "grid", "--size", "40x12"], None);
    let (_, ps) = s.panes("grid");
    let a = id(&ps[0]);
    let b = s.ok(&["pane", "split", "grid", "--horizontal"], None)["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    let c = s.ok(&["pane", "split", b.as_str()], None)["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    // a | (b / c)
    let sel = |from: &str, dir: &str| -> String {
        s.ok(&["pane", "select", from, dir], Some("result_select"))["pane_id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(sel(&a, "right"), b);
    assert_eq!(sel(&c, "left"), a);
    assert_eq!(sel(&b, "down"), c);
    assert_eq!(sel(&c, "up"), b);
    assert_eq!(sel(&b, "right"), a, "wraps around");
    assert_eq!(sel(&c, "next"), a, "next wraps by index");
    assert_eq!(sel(&a, "previous"), c);
    // A Terminal target means its active pane.
    s.ok(&["pane", "select", c.as_str()], Some("result_select"));
    assert_eq!(sel("grid", "up"), b);

    // Dry runs report and change nothing.
    let before = s.panes("grid").0["layout"].clone();
    let d = s.ok(
        &["pane", "split", a.as_str(), "--cells", "5", "--dry-run"],
        Some("result_window_create"),
    );
    assert_eq!(d["would_create"]["size"], json!({"cols": 20, "rows": 5}));
    assert_eq!(d["would_create"]["split"]["pane_id"], a.as_str());
    let d = s.ok(
        &["pane", "resize", a.as_str(), "right", "3", "--dry-run"],
        Some("result_layout"),
    );
    assert_eq!(d["would_change"]["moved"], 3);
    s.ok(
        &["pane", "zoom", a.as_str(), "--dry-run"],
        Some("result_layout"),
    );
    assert_eq!(
        s.panes("grid").0["layout"],
        before,
        "dry runs change nothing"
    );
    assert_eq!(s.panes("grid").1.len(), 3);

    // Splitting stops at the minimum (2 columns, 2 rows per pane): refused, nothing changes.
    let mut refused = None;
    for _ in 0..8 {
        let (exit, doc) = s.cli(&["pane", "split", c.as_str()]);
        if exit != 0 {
            refused = Some(doc);
            break;
        }
        remember(&s.seen, &doc["data"]);
    }
    let doc = refused.expect("a 12-row window cannot be split forever");
    assert_eq!(doc["error"]["code"], "invalid_state", "{doc}");
    let details = doc["error"]["details"].to_string();
    assert!(
        details.contains("need: ") && details.contains("have: "),
        "{doc}"
    );
    assert!(doc["error"]["hint"].as_str().unwrap().contains("bigger"));
    let (_, ps) = s.panes("grid");
    for p in &ps {
        let (_, _, cols, rows) = rect(p);
        assert!(cols >= 2 && rows >= 2, "{p}");
        assert!(alive(pid_of(&p["pid"])));
    }

    // Bad requests.
    s.fails(&["pane", "resize", a.as_str(), "sideways"], "usage");
    s.fails(
        &["pane", "split", a.as_str(), "--horizontal", "--vertical"],
        "usage",
    );
    s.fails(&["pane", "split", a.as_str(), "--percent", "100"], "usage");
    s.fails(&["pane", "zoom", a.as_str(), "maybe"], "usage");
    s.fails(&["pane", "select", a.as_str(), "around"], "usage");
    s.fails(&["pane", "swap", a.as_str(), "nowhere"], "target_not_found");
}

/// SUBAK-FR-013/014: the layout belongs to the Host. A client's size is its window's (the
/// latest interactive client's), `window.layout` tells every client, and the layout stays as
/// it is when clients detach, crash or reconnect.
#[test]
fn the_layout_belongs_to_the_host_across_clients() {
    let s = Session::start("w22-clients");
    s.ok(&["new", "keep", "--size", "80x24"], None);
    let watcher = Raw::handshake(&s.ep, "cli");
    let r = watcher.request(1, "events.subscribe", &json!({"topics": ["terminals"]}));
    assert_eq!(r["ok"], true, "{r}");
    let b = s.ok(&["pane", "split", "keep", "--horizontal"], None)["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    let got = events_until(&watcher, |e| !layouts(e).is_empty());
    let l = layouts(&got)[0];
    assert_eq!(l["size"], json!({"cols": 80, "rows": 24}));
    assert_eq!(l["panes"].as_array().unwrap().len(), 2);

    // A window client attaches at 101x30: the window takes its size; both panes are resized.
    let gui = Raw::handshake(&s.ep, "gui");
    let att = gui.request(
        1,
        "pane.attach",
        &json!({"target": "keep", "interactive": true, "size": {"cols": 101, "rows": 30}}),
    );
    assert_eq!(att["ok"], true, "{att}");
    assert_eq!(att["result"]["pane_id"], b.as_str(), "the active pane");
    let got = events_until(&watcher, |e| {
        layouts(e)
            .iter()
            .any(|l| l["size"] == json!({"cols": 101, "rows": 30}))
    });
    assert!(!got.is_empty());
    let (w, ps) = s.panes("keep");
    assert_eq!(w["size"], json!({"cols": 101, "rows": 30}));
    assert_eq!(rect(&ps[0]), (0, 0, 50, 30));
    assert_eq!(rect(&ps[1]), (51, 0, 50, 30));
    // The client's own resize keeps working: the reply is the pane's share of the window.
    let rz = gui.request(
        2,
        "pane.resize",
        &json!({"cols": 121, "rows": 30, "stream_id": att["result"]["stream_id"]}),
    );
    assert_eq!(rz["ok"], true, "{rz}");
    assert_eq!(rz["result"]["cols"], 60);
    assert_eq!(
        rz["result"]["window_size"],
        json!({"cols": 121, "rows": 30})
    );
    let pids: Vec<u32> = ps.iter().map(|p| pid_of(&p["pid"])).collect();
    let layout = s.panes("keep").0["layout"].clone();

    // The window crashes (its connection drops) and a viewer reconnects read-only: the layout
    // is unchanged and nothing ended.
    drop(gui);
    std::thread::sleep(Duration::from_millis(300));
    let viewer = Raw::handshake(&s.ep, "gui");
    let v = viewer.request(
        1,
        "pane.attach",
        &json!({"target": "keep", "interactive": false, "size": {"cols": 20, "rows": 5}}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(
        s.panes("keep").0["layout"],
        layout,
        "a viewer never resizes"
    );
    // The latest interactive client decides: a small one makes the panes small, never below
    // their minimum.
    let small = Raw::handshake(&s.ep, "gui");
    let r = small.request(
        1,
        "pane.attach",
        &json!({"target": "keep", "interactive": true, "size": {"cols": 4, "rows": 3}}),
    );
    assert_eq!(r["ok"], true, "{r}");
    let (w, ps) = s.panes("keep");
    assert_eq!(
        w["size"],
        json!({"cols": 5, "rows": 3}),
        "the layout's minimum"
    );
    assert_eq!(rect(&ps[0]).2, 2);
    for p in &pids {
        assert!(alive(*p), "pid {p} must survive every client change");
    }
}

/// `rotate-window` moves the panes through the places, `select-layout` arranges them evenly;
/// every process keeps running and is resized to its new place.
#[test]
fn rotate_and_basic_layouts_rearrange_without_ending_anything() {
    let s = Session::start("w22-rotate");
    let mut args = vec!["new", "rot", "--size", "81x24"];
    args.extend_from_slice(REPORTER);
    s.ok(&args, None);
    let a = id(&s.panes("rot").1[0]);
    let mut sp = vec!["pane", "split", "rot", "--horizontal", "--cells", "20"];
    sp.extend_from_slice(REPORTER);
    let b = s.ok(&sp, None)["pane_id"].as_str().unwrap().to_string();
    let (_, ps) = s.panes("rot");
    let pids: Vec<u32> = ps.iter().map(|p| pid_of(&p["pid"])).collect();
    s.reports(&a, 60, 24);
    s.reports(&b, 20, 24);
    // Rotate: the panes swap places (two panes); sizes follow the places.
    let d = s.ok(&["window", "rotate", "rot"], Some("result_layout"));
    assert_eq!(d["changed"], true);
    let (_, ps) = s.panes("rot");
    assert_eq!(id(&ps[0]), b);
    s.reports(&b, 60, 24);
    s.reports(&a, 20, 24);
    s.ok(&["rotate-window", "rot", "down"], Some("result_layout"));
    assert_eq!(id(&s.panes("rot").1[0]), a);
    // Even layouts.
    s.ok(
        &["window", "layout", "rot", "even-horizontal"],
        Some("result_layout"),
    );
    s.reports(&a, 40, 24);
    s.reports(&b, 40, 24);
    let d = s.ok(
        &["select-layout", "rot", "even-vertical"],
        Some("result_layout"),
    );
    assert_eq!(d["layout"]["direction"], "vertical");
    s.reports(&a, 81, 11);
    s.reports(&b, 81, 12);
    let d = s.ok(
        &["window", "layout", "rot", "tiled", "--dry-run"],
        Some("result_layout"),
    );
    assert_eq!(d["would_change"]["changed"], true);
    s.fails(&["window", "layout", "rot", "spiral"], "usage");
    s.fails(&["window", "rotate", "rot", "sideways"], "usage");
    for p in pids {
        assert!(alive(p));
    }
}

/// Repeated deep splits on a big window stay fast and consistent; every split's pane runs.
#[test]
fn repeated_deep_splits_stay_fast_and_consistent() {
    let s = Session::start("w22-deep");
    s.ok(&["new", "deep", "--size", "240x64"], None);
    let mut target = id(&s.panes("deep").1[0]);
    let mut times = Vec::new();
    for i in 0..12 {
        let dir = if i % 2 == 0 {
            "--horizontal"
        } else {
            "--vertical"
        };
        let t0 = Instant::now();
        let (exit, doc) = s.cli(&["pane", "split", target.as_str(), dir]);
        times.push(t0.elapsed());
        if exit != 0 {
            assert_eq!(doc["error"]["code"], "invalid_state", "{doc}");
            break;
        }
        remember(&s.seen, &doc["data"]);
        target = doc["data"]["pane_id"].as_str().unwrap().to_string();
    }
    let (w, ps) = s.panes("deep");
    assert!(ps.len() >= 9, "{} panes", ps.len());
    // Every cell is covered once: panes plus borders never overlap.
    for (i, p) in ps.iter().enumerate() {
        let (x, y, c, r) = rect(p);
        assert!(x + c <= 240 && y + r <= 64);
        for q in ps.iter().skip(i + 1) {
            let (qx, qy, qc, qr) = rect(q);
            assert!(
                x + c <= qx || qx + qc <= x || y + r <= qy || qy + qr <= y,
                "{p} overlaps {q}"
            );
        }
        assert!(alive(pid_of(&p["pid"])));
    }
    assert_eq!(w["layout"]["type"], "split");
    times.sort();
    println!(
        "EVIDENCE pane-split: {} splits of a 240x64 window; CLI round trip with a new cmd.exe pane p50 {} ms, max {} ms",
        times.len(),
        times[times.len() / 2].as_millis(),
        times.last().unwrap().as_millis()
    );
}

//! E2E — Step 22: pane splits and the layout engine (docs/SESSION_MODEL.md §3, ADR 0056).
//! `SUBAK-FR-018`, `SUBAK-FR-013`, `SUBAK-FR-017`.
//!
//! Scenario A (release `subak`): a user splits one Terminal side by side and stacked, with
//! programs that print their console size; each program sees exactly its share of the window
//! after every split, border move, zoom, swap, rotate, even layout and close, and every other
//! process keeps running (`tasklist`, independent of subak, sees them).
//!
//! Scenario B (release `subak`): a pane too small to split is refused and nothing changes; dry
//! runs change nothing.
//!
//! Windows only. Every scenario uses its own endpoint and stops what it started; nothing opens a
//! window.
#![cfg(windows)]

use kim_e2e::{TempDir, bin};
use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prints `SZ <cols>x<rows>` four times a second.
const REPORTER: &[&str] = &[
    "--",
    "powershell.exe",
    "-NoLogo",
    "-NoProfile",
    "-Command",
    "while($true){'SZ '+[Console]::WindowWidth+'x'+[Console]::WindowHeight; Start-Sleep -Milliseconds 250}",
];

fn endpoint(tag: &str) -> String {
    format!("subak-e2e22-{tag}-{}", std::process::id())
}

fn json(exe: &Path, ep: &str, args: &[&str], sb: &TempDir) -> (i32, Value) {
    let out = Command::new(exe)
        .args(["--json", "--endpoint", ep])
        .args(args)
        .env("USERPROFILE", sb.path())
        .env("APPDATA", sb.path())
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

fn ok(exe: &Path, ep: &str, args: &[&str], sb: &TempDir) -> Value {
    let (code, doc) = json(exe, ep, args, sb);
    assert_eq!(code, 0, "{args:?}: {doc}");
    doc["data"].clone()
}

fn with_reporter<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut v = args.to_vec();
    v.extend_from_slice(REPORTER);
    v
}

fn running(pid: u64) -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}

/// The panes of a Terminal's first window as (id, pid, rect).
fn panes(exe: &Path, ep: &str, t: &str, sb: &TempDir) -> (Value, Vec<(String, u64, Value)>) {
    let d = ok(exe, ep, &["pane", "ls", t], sb);
    let w = d["windows"][0].clone();
    let ps = w["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_string(),
                p["pid"].as_u64().unwrap_or(0),
                p["rect"].clone(),
            )
        })
        .collect();
    (w, ps)
}

/// Waits until the pane's program reports `SZ <cols>x<rows>` as its newest line.
fn sees(exe: &Path, ep: &str, pane: &str, sb: &TempDir, cols: u64, rows: u64) {
    let want = format!("SZ {cols}x{rows}");
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        let d = ok(exe, ep, &["capture", pane], sb);
        let last = d["lines"]
            .as_array()
            .and_then(|l| {
                l.iter()
                    .filter_map(Value::as_str)
                    .rfind(|s| s.starts_with("SZ "))
                    .map(|s| s.trim().to_string())
            })
            .unwrap_or_default();
        if last == want {
            return;
        }
        assert!(
            Instant::now() < until,
            "pane {pane} sees `{last}`, not `{want}`"
        );
        std::thread::sleep(Duration::from_millis(150));
    }
}

struct Cleanup<'a> {
    exe: &'a Path,
    ep: String,
    sb: &'a TempDir,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = json(
            self.exe,
            &self.ep,
            &["host", "stop", "--kill-terminals", "--yes"],
            self.sb,
        );
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_user_splits_resizes_zooms_and_rearranges_panes_and_every_program_sees_its_size() {
    kim_e2e::require_e2e();
    let exe = bin("subak");
    let sb = TempDir::new("e2e22-split");
    let ep = endpoint("split");
    let _c = Cleanup {
        exe: &exe,
        ep: ep.clone(),
        sb: &sb,
    };
    ok(
        &exe,
        &ep,
        &with_reporter(&["new", "work", "--size", "81x24"]),
        &sb,
    );
    let a = panes(&exe, &ep, "work", &sb).1[0].0.clone();
    sees(&exe, &ep, &a, &sb, 81, 24);
    // Side by side, then stacked on the right.
    let b = ok(
        &exe,
        &ep,
        &with_reporter(&["pane", "split", "work", "--horizontal"]),
        &sb,
    )["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    sees(&exe, &ep, &a, &sb, 40, 24);
    sees(&exe, &ep, &b, &sb, 40, 24);
    let c = ok(
        &exe,
        &ep,
        &with_reporter(&["split-window", b.as_str()]),
        &sb,
    )["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    sees(&exe, &ep, &b, &sb, 40, 12);
    sees(&exe, &ep, &c, &sb, 40, 11);
    let pids: Vec<u64> = panes(&exe, &ep, "work", &sb)
        .1
        .iter()
        .map(|p| p.1)
        .collect();
    assert!(pids.iter().all(|p| running(*p)));
    // A border move, a zoom and back.
    let d = ok(
        &exe,
        &ep,
        &["pane", "resize", a.as_str(), "right", "6"],
        &sb,
    );
    assert_eq!(d["moved"], 6);
    sees(&exe, &ep, &a, &sb, 46, 24);
    sees(&exe, &ep, &c, &sb, 34, 11);
    ok(&exe, &ep, &["pane", "zoom", c.as_str()], &sb);
    sees(&exe, &ep, &c, &sb, 81, 24);
    ok(&exe, &ep, &["pane", "zoom", c.as_str(), "off"], &sb);
    sees(&exe, &ep, &c, &sb, 34, 11);
    // Swap, rotate and an even layout: processes move with their panes.
    ok(&exe, &ep, &["pane", "swap", a.as_str(), c.as_str()], &sb);
    sees(&exe, &ep, &c, &sb, 46, 24);
    ok(&exe, &ep, &["rotate-window", "work"], &sb);
    ok(
        &exe,
        &ep,
        &["window", "layout", "work", "even-horizontal"],
        &sb,
    );
    // 79 columns for three panes: 26, 26 and 27, each program seeing its own.
    for (id, _, r) in panes(&exe, &ep, "work", &sb).1 {
        let cols = r["cols"].as_u64().unwrap();
        assert!((26..=27).contains(&cols), "{id}: {r}");
        sees(&exe, &ep, &id, &sb, cols, 24);
    }
    // Closing a pane gives its space to its neighbour; the others keep running.
    ok(&exe, &ep, &["pane", "kill", b.as_str(), "--yes"], &sb);
    let (_, ps) = panes(&exe, &ep, "work", &sb);
    assert_eq!(ps.len(), 2);
    let widths: u64 = ps.iter().map(|p| p.2["cols"].as_u64().unwrap()).sum();
    assert_eq!(widths + 1, 81, "two panes and a border fill the window");
    for (id, pid, _) in &ps {
        assert!(running(*pid), "{id} keeps running");
    }
    let until = Instant::now() + Duration::from_secs(10);
    while running(pids[1]) {
        assert!(Instant::now() < until, "the killed pane's process ended");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn refusals_and_dry_runs_change_nothing() {
    kim_e2e::require_e2e();
    let exe = bin("subak");
    let sb = TempDir::new("e2e22-refuse");
    let ep = endpoint("refuse");
    let _c = Cleanup {
        exe: &exe,
        ep: ep.clone(),
        sb: &sb,
    };
    ok(&exe, &ep, &["new", "tiny", "--size", "12x5"], &sb);
    ok(&exe, &ep, &["pane", "split", "tiny"], &sb);
    let (w, ps) = panes(&exe, &ep, "tiny", &sb);
    // 2 rows each: no room for another stacked pane.
    let (code, doc) = json(&exe, &ep, &["pane", "split", ps[1].0.as_str()], &sb);
    assert_eq!(
        (code, doc["error"]["code"].as_str()),
        (7, Some("invalid_state")),
        "{doc}"
    );
    let d = ok(
        &exe,
        &ep,
        &[
            "pane",
            "split",
            ps[0].0.as_str(),
            "--horizontal",
            "--dry-run",
        ],
        &sb,
    );
    assert_eq!(d["dry_run"], true);
    ok(
        &exe,
        &ep,
        &["window", "layout", "tiny", "tiled", "--dry-run"],
        &sb,
    );
    let (after, ps2) = panes(&exe, &ep, "tiny", &sb);
    assert_eq!(
        after["layout"], w["layout"],
        "refusals and dry runs change nothing"
    );
    assert_eq!(ps2.len(), 2);
    for (_, pid, _) in &ps2 {
        assert!(running(*pid));
    }
}

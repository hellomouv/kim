//! E2E — Step 39: resource counters, idle cost and a create/remove soak with the release
//! `subak` (docs/PERFORMANCE.md). `KIM-NFR-001`.
//!
//! `subak host stats` reports memory, handles and CPU; ten idle Terminals cost (almost) no CPU
//! over five seconds; thirty create/remove cycles grow the handle count by at most what
//! Windows 10's inbox ConPTY keeps per pane (one) plus a small allowance.
#![cfg(windows)]

use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn the_host_reports_its_costs_and_stays_flat_over_a_soak() {
    let id = std::process::id();
    let ep = format!("subak-e2e39-{id}");
    let appdata = std::env::temp_dir().join(format!("subak-e2e39-{id}"));
    std::fs::create_dir_all(&appdata).unwrap();
    let exe = kim_e2e::bin("subak");
    let run = |args: &[&str]| -> Value {
        let out = Command::new(&exe)
            .args(["--json", "--endpoint", &ep])
            .args(args)
            .env("APPDATA", &appdata)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["ok"], true, "{args:?}: {v}");
        v["data"].clone()
    };
    run(&["host", "start", "--no-shell"]);
    for i in 0..10 {
        run(&["new", &format!("idle{i}")]);
    }
    std::thread::sleep(Duration::from_secs(2));
    let cpu = |s: &Value| {
        s["host"]["cpu"]["kernel_ms"].as_u64().unwrap()
            + s["host"]["cpu"]["user_ms"].as_u64().unwrap()
    };
    let a = run(&["host", "stats"]);
    assert!(a["host"]["memory"]["private_bytes"].as_u64().unwrap() > 0);
    std::thread::sleep(Duration::from_secs(5));
    let b = run(&["host", "stats"]);
    assert!(
        cpu(&b) - cpu(&a) <= 50,
        "idle CPU {} ms in 5 s",
        cpu(&b) - cpu(&a)
    );
    let h0 = b["host"]["handle_count"].as_i64().unwrap();
    for i in 0..30 {
        let n = format!("soak{i}");
        run(&["new", &n]);
        run(&["rm", &n, "--force", "--yes"]);
    }
    std::thread::sleep(Duration::from_secs(2));
    let h1 = run(&["host", "stats"])["host"]["handle_count"]
        .as_i64()
        .unwrap();
    assert!(
        h1 - h0 <= 30 + 16,
        "handles grew by {} over 30 cycles",
        h1 - h0
    );
    run(&["host", "stop", "--kill-terminals", "--yes"]);
    let _ = std::fs::remove_dir_all(&appdata);
}

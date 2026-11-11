//! Milestone C (Step 31) — recovery evidence that can run unattended on this PC
//! (docs/MILESTONE_C_RECOVERY_VALIDATION.md). `SUBAK-FR-040`, `SUBAK-FR-045`, `SUBAK-SEC-002`,
//! `SUBAK-SEC-003`.
//!
//! A Host owning real cmd.exe and PowerShell panes, a long-running job and a multi-window
//! topology **crashes** (TerminateProcess, no clean stop). Its panes end with it (the job object;
//! nothing claims to survive), the last coalesced `state.json` is intact, and a new Host
//! rebuilds the topology on request — timed — with fresh shells, without re-running the job,
//! never twice. Power transitions (lock, sleep, hibernate, reboot, power loss) need the
//! maintainer: `scripts/recovery-matrix.ps1` runs their phases.
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
    let mut a = vec!["--json", "--endpoint", ep];
    a.extend_from_slice(args);
    let (code, doc) = run_env(&a, &[]);
    validate("subak-cli-v1", &doc);
    assert_eq!(code, 0, "{args:?}: {doc}");
    doc["data"].clone()
}

fn pids(ep: &str) -> Vec<u32> {
    cli(ep, &["ls"])["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| {
            let d = cli(ep, &["pane", "ls", t["id"].as_str().unwrap()]);
            d["windows"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|w| {
                    w["panes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|p| p["pid"].as_u64())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .filter_map(|p| u32::try_from(p).ok())
        .collect()
}

#[test]
fn a_crashed_host_leaves_an_intact_state_and_a_cold_rebuild_starts_nothing_twice() {
    let dir = std::env::temp_dir().join(format!("subak-milestone-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sd = dir.display().to_string();
    let ep = endpoint("milestone-c");
    let start = |ep: &str| {
        let (code, doc) = run(&[
            "host",
            "start",
            "--endpoint",
            ep,
            "--no-shell",
            "--idle-exit",
            "60",
            "--state-dir",
            &sd,
        ]);
        assert_eq!(code, 0, "{doc}");
        Proc::open(pid_of(&doc["data"]["host"]["pid"]))
    };
    let host = start(&ep);
    let marker = dir.join("job-ran.txt");
    let job = format!(
        "cmd.exe /d /c \"echo ran>>\"{}\" & ping -n 600 127.0.0.1 >nul\"",
        marker.display()
    );
    cli(&ep, &["new", "dev", "--size", "140x40"]);
    cli(
        &ep,
        &[
            "command",
            "--target",
            "dev",
            "split-window -h; new-window -n ps",
        ],
    );
    cli(
        &ep,
        &["new", "ps", "--", "powershell.exe", "-NoLogo", "-NoProfile"],
    );
    cli(&ep, &["new", "job", "--command-line", &job]);
    let before_topology = cli(&ep, &["pane", "ls", "dev"]);
    let end = Instant::now() + LONG;
    while !marker.exists() {
        assert!(Instant::now() < end, "the job did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    // Let the coalesced write happen, then crash the Host.
    let state = dir.join("state.json");
    let end = Instant::now() + LONG;
    loop {
        let ok = std::fs::read_to_string(&state)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .is_some_and(|v| v["terminals"].as_array().is_some_and(|t| t.len() == 3));
        if ok {
            break;
        }
        assert!(
            Instant::now() < end,
            "state.json never held the three Terminals"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let live = pids(&ep);
    assert_eq!(live.len(), 5, "{live:?}");
    host.kill();
    assert!(host.wait(Duration::from_secs(10)));
    // Every pane process ended with its Host: nothing claims to survive.
    let end = Instant::now() + LONG;
    while live
        .iter()
        .any(|p| Proc::try_open(*p).is_some_and(|p| p.alive()))
    {
        assert!(
            Instant::now() < end,
            "a pane outlived its crashed Host: {live:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let (code, doc) = run_env(&["--json", "state", "check", "--state-dir", &sd], &[]);
    assert_eq!(code, 0, "the last write is intact after a crash: {doc}");
    std::fs::remove_file(&marker).unwrap();

    // A new Host: nothing runs until asked; then the topology is back, timed.
    let host = start(&ep);
    assert_eq!(cli(&ep, &["ls"])["terminals"], json!([]));
    let list = cli(&ep, &["recover", "list"]);
    assert_eq!(list["terminals"].as_array().unwrap().len(), 3);
    let t0 = Instant::now();
    for t in ["dev", "ps", "job"] {
        let r = cli(&ep, &["recover", t, "--yes"]);
        assert_eq!(r["failed"], json!([]), "{t}: {r}");
    }
    let took = t0.elapsed();
    let after = cli(&ep, &["pane", "ls", "dev"]);
    assert_eq!(after["terminal_id"], before_topology["terminal_id"]);
    assert_eq!(after["windows"].as_array().unwrap().len(), 2);
    assert_eq!(after["windows"][0]["panes"].as_array().unwrap().len(), 2);
    let fresh = pids(&ep);
    assert_eq!(fresh.len(), 5);
    assert!(
        fresh.iter().all(|p| !live.contains(p)),
        "new processes, new pids"
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "the job was not re-run");
    let (code, _) = run_env(
        &["--json", "--endpoint", &ep, "recover", "dev", "--yes"],
        &[],
    );
    assert_eq!(code, 7, "never twice");
    println!(
        "EVIDENCE milestone-c: crashed Host with 5 panes (cmd, PowerShell, a job) -> all ended, state.json intact; cold rebuild of 3 Terminals / 5 panes in {} ms; job not re-run; duplicate refused",
        took.as_millis()
    );
    let _ = run(&[
        "host",
        "stop",
        "--endpoint",
        &ep,
        "--kill-terminals",
        "--yes",
    ]);
    assert!(host.wait(Duration::from_secs(20)));
    let _ = std::fs::remove_dir_all(&dir);
}

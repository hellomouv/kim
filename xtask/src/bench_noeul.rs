//! `cargo xtask bench-noeul [--record|--check]` (Step 63, docs/PERFORMANCE.md §5): Noeul's
//! resource and latency budgets (NOEUL-NFR-001…006) measured on the release build.
//!
//! - binary size; `noeul status` without an instance (cold CLI);
//! - the instance: `noeul run --no-tray --no-hotkeys` in a private profile and endpoint (no window,
//!   no tray icon, no hotkey) — launch to healthy, then idle CPU over 10 s and private memory;
//! - the overlay's first frame and the double-click-to-pin latency through the instance core on
//!   **synthetic** off-screen monitors (`overlay_measure --synthetic-only`): nothing is read from
//!   the screen and no window is visible.
//!
//! `--record` writes `docs/perf/noeul-baseline.json`; `--check` fails when a budget is exceeded.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn no_window(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn pct(mut v: Vec<f64>, p: usize) -> f64 {
    v.sort_by(f64::total_cmp);
    let rank = (p * v.len()).div_ceil(100).max(1);
    v[rank - 1]
}

/// CPU time (ms) and private bytes of a process, from PowerShell's `Get-Process` (no window).
fn proc_sample(pid: u32) -> Result<(f64, f64), String> {
    let out = no_window(Command::new("powershell").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!("$p = Get-Process -Id {pid}; \"$($p.TotalProcessorTime.TotalMilliseconds) $($p.PrivateMemorySize64)\""),
    ]))
    .output()
    .map_err(|e| e.to_string())?;
    let t = String::from_utf8_lossy(&out.stdout);
    let mut it = t
        .split_whitespace()
        .map(|s| s.replace(',', ".").parse::<f64>());
    match (it.next(), it.next()) {
        (Some(Ok(cpu)), Some(Ok(mem))) => Ok((cpu, mem)),
        _ => Err(format!("cannot read process {pid}: {t}")),
    }
}

fn measure(root: &Path) -> Result<Value, String> {
    let rel = root.join("target").join("release");
    let exe = rel.join("noeul.exe");
    let sandbox = std::env::temp_dir().join(format!("noeul-bench-{}", std::process::id()));
    std::fs::create_dir_all(sandbox.join("Roaming")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(sandbox.join("Local")).map_err(|e| e.to_string())?;
    let ep = format!("noeul-bench-{}", std::process::id());
    let cmd = |args: &[&str]| {
        let mut c = Command::new(&exe);
        c.args(args)
            .args(["--endpoint", &ep])
            .env("APPDATA", sandbox.join("Roaming"))
            .env("LOCALAPPDATA", sandbox.join("Local"))
            .env("KIM_NO_INPUT", "1")
            .env_remove("NOEUL_DEV_SYNTHETIC_DESKTOP");
        no_window(&mut c);
        c
    };
    let size = std::fs::metadata(&exe).map_err(|e| e.to_string())?.len();
    // Cold CLI.
    let mut cli = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        let o = cmd(&["status", "--json"])
            .output()
            .map_err(|e| e.to_string())?;
        if !o.status.success() {
            return Err("noeul status failed".into());
        }
        cli.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    // The instance: no tray, no hotkeys, nothing visible.
    let t = Instant::now();
    let mut child = cmd(&["run", "--no-tray", "--no-hotkeys"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let ready = loop {
        if cmd(&["health", "--json"])
            .output()
            .is_ok_and(|o| o.status.success())
        {
            break t.elapsed().as_secs_f64() * 1000.0;
        }
        if t.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            return Err("the instance never became healthy".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    std::thread::sleep(Duration::from_secs(2));
    let (cpu0, _) = proc_sample(child.id())?;
    std::thread::sleep(Duration::from_secs(10));
    let (cpu1, private) = proc_sample(child.id())?;
    let _ = cmd(&["quit", "--yes", "--json"]).output();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&sandbox);
    // The overlay and the double-click, on synthetic monitors.
    let example = |args: &[&str]| -> Result<Value, String> {
        let o = no_window(
            Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .args([
                    "run",
                    "--release",
                    "-q",
                    "-p",
                    "noeul",
                    "--example",
                    "overlay_measure",
                    "--",
                ])
                .args(args)
                .current_dir(root),
        )
        .output()
        .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&o.stdout);
        text.lines()
            .rev()
            .find_map(|l| serde_json::from_str::<Value>(l).ok())
            .ok_or_else(|| format!("overlay_measure printed no JSON: {text}"))
    };
    let ov = example(&["--synthetic-only", "--runs", "10", "--json"])?;
    let dc = example(&["--double-click", "--runs", "20", "--json"])?;
    let layout = |name: &str| {
        ov["overlay"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["layout"] == name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let ms = |v: &Value| r1(v.as_f64().unwrap_or(f64::NAN) / 1000.0);
    let mib = |v: &Value| r1(v.as_f64().unwrap_or(f64::NAN) / 1_048_576.0);
    let (uhd, three) = (layout("4k-150"), layout("3x4k"));
    Ok(json!({
        "binary_mib": r1(size as f64 / 1_048_576.0),
        "cli_status_ms": {"median": r1(pct(cli.clone(), 50)), "p95": r1(pct(cli, 95))},
        "instance_ready_ms": r1(ready),
        "idle_cpu_ms_per_10s": r1(cpu1 - cpu0),
        "idle_private_mib": r1(private / 1_048_576.0),
        "overlay_first_frame_p95_ms": {"4k": ms(&uhd["first_frame_us"]["p95"]), "3x4k": ms(&three["first_frame_us"]["p95"])},
        "overlay_memory_back_within_mib": r1(mib(&uhd["private_bytes"]["after_10s"]) - mib(&uhd["private_bytes"]["before"])),
        "double_click_pin_p95_ms": {"1080p": ms(&dc["double_click_pin_us"]["1920x1080"]["p95"]), "4k": ms(&dc["double_click_pin_us"]["3840x2160"]["p95"])},
    }))
}

/// The budgets of NOEUL-NFR-001…006: (path, limit, requirement).
const BUDGETS: [(&str, f64, &str); 9] = [
    ("/binary_mib", 12.0, "NOEUL-NFR-006"),
    ("/instance_ready_ms", 300.0, "NOEUL-NFR-006"),
    (
        "/idle_cpu_ms_per_10s",
        10.0,
        "NOEUL-NFR-001 (0.1 % of one core)",
    ),
    ("/idle_private_mib", 20.0, "NOEUL-NFR-002"),
    ("/overlay_first_frame_p95_ms/4k", 150.0, "NOEUL-NFR-003"),
    ("/overlay_first_frame_p95_ms/3x4k", 250.0, "NOEUL-NFR-003"),
    ("/overlay_memory_back_within_mib", 5.0, "NOEUL-NFR-002"),
    ("/double_click_pin_p95_ms/1080p", 50.0, "NOEUL-NFR-004"),
    ("/double_click_pin_p95_ms/4k", 100.0, "NOEUL-NFR-004"),
];

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("bench-noeul measures the Windows build; run it on Windows".into());
    }
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "-p", "noeul", "--bins", "--examples"])
        .current_dir(root)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("cargo build --release -p noeul failed".into());
    }
    let m = measure(root)?;
    println!("{}", serde_json::to_string_pretty(&m).unwrap_or_default());
    let mut over = Vec::new();
    for (path, limit, req) in BUDGETS {
        let v = m
            .pointer(path)
            .and_then(Value::as_f64)
            .unwrap_or(f64::INFINITY);
        let ok = v <= limit;
        println!(
            "bench-noeul: {path} = {v} (budget {limit}, {req}) {}",
            if ok { "ok" } else { "OVER" }
        );
        if !ok {
            over.push(path);
        }
    }
    if args.iter().any(|a| a == "--record") {
        let doc = json!({"schema": "kim.bench.noeul", "schema_version": 1,
            "recorded": std::env::var("COMPUTERNAME").map(|_| "this machine").unwrap_or("unknown"),
            "machine": {"cpu": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),
                "processors": std::env::var("NUMBER_OF_PROCESSORS").unwrap_or_default()},
            "measured": m,
            "budgets": BUDGETS.iter().map(|(p, l, r)| json!({"path": p, "limit": l, "requirement": r})).collect::<Vec<_>>()});
        let p = root.join("docs").join("perf").join("noeul-baseline.json");
        std::fs::write(
            &p,
            serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n",
        )
        .map_err(|e| e.to_string())?;
        println!("bench-noeul: recorded {}", p.display());
    }
    if args.iter().any(|a| a == "--check") && !over.is_empty() {
        return Err(format!("over budget: {over:?}"));
    }
    Ok(())
}

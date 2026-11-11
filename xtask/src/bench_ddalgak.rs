//! `cargo xtask bench-ddalgak [--record|--check]` (Step 82, docs/PERFORMANCE.md §6): Ddalgak's
//! resource and latency budgets (DDALGAK-NFR-001…004) measured without anything visible.
//!
//! - the release binaries' sizes; `ddalgak rule check` on 1000 rules (load and validate);
//! - the app as the debug **test host** (`ddalgak-gui --test-host`: no tray icon, a real
//!   keyboard hook that never consumes a key) in a private profile and endpoint, with 0, 10, 100
//!   and 1000 active rules: launch to running with the hook installed, then idle CPU, wakeups and
//!   memory over 10 s; the debug build is slower than the release one, so these are upper bounds;
//! - the hook callback's p99 after a burst of 400 F20 taps sent as another program
//!   (`examples/hook_burst.rs`; skipped on a locked desktop); F20 is bound by no rule;
//! - a profile switch of 1000 rules in the running app (`profile apply`, saved and reloaded).
//!
//! `--record` writes `docs/perf/ddalgak-baseline.json`; `--check` fails when a budget is exceeded.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
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

fn mib(bytes: u64) -> f64 {
    r1(bytes as f64 / 1_048_576.0)
}

/// Working set and private bytes of a process (PowerShell, no window).
fn memory(pid: u32) -> (f64, f64) {
    let out = no_window(Command::new("powershell").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!("$p = Get-Process -Id {pid}; \"$($p.WorkingSet64) $($p.PrivateMemorySize64)\""),
    ]))
    .output();
    let t = out
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let v: Vec<u64> = t
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    (
        mib(v.first().copied().unwrap_or(0)),
        mib(v.get(1).copied().unwrap_or(0)),
    )
}

/// `n` active engine rules with distinct three-step sequence triggers (no consent needed; none
/// is ever pressed) in profile `bench`, and an empty profile `other`.
fn rules_doc(n: usize) -> Value {
    let letters: Vec<char> = ('A'..='Z').collect();
    let rules: Vec<Value> = (0..n)
        .map(|i| {
            let f = 13 + i % 12;
            let a = letters[(i / 12) % 26];
            let b = letters[(i / 312) % 26];
            json!({"id": format!("r{i}"), "title": format!("Rule {i}"), "enabled": true,
                "trigger": {"keys": format!("Ctrl+Alt+Shift+F{f} {a} {b}")},
                "steps": [{"type": "engine", "op": "toggle_pause"}]})
        })
        .collect();
    json!({"schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 1,
    "active_profile": "bench", "settings": {"engine_on_start": "on"},
    "profiles": [
        {"id": "bench", "title": "Bench", "default_mode": "base", "modes": [{"id": "base", "title": "Base"}], "rules": rules},
        {"id": "other", "title": "Other", "default_mode": "base", "modes": [{"id": "base", "title": "Base"}], "rules": []}
    ]})
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Sandbox {
        let root = std::env::temp_dir().join(format!("ddalgak-bench-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Roaming").join("mouv").join("ddalgak"))
            .unwrap_or_default();
        std::fs::create_dir_all(root.join("Local")).unwrap_or_default();
        Sandbox { root }
    }
    fn cmd(&self, exe: &Path) -> Command {
        let mut c = Command::new(exe);
        c.env("APPDATA", self.root.join("Roaming"))
            .env("LOCALAPPDATA", self.root.join("Local"));
        no_window(&mut c);
        c
    }
    fn write(&self, doc: &Value) {
        let _ = std::fs::write(
            self.root.join("Roaming/mouv/ddalgak/rules.json"),
            doc.to_string(),
        );
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn cli(sb: &Sandbox, exe: &Path, endpoint: &str, args: &[&str]) -> Value {
    let o = sb
        .cmd(exe)
        .args(args)
        .args(["--endpoint", endpoint, "--json"])
        .output();
    o.ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or(Value::Null)
}

/// One test host with `n` rules: (ready ms, idle cpu ms/10 s, wakeups/10 s, working set, private).
fn host_run(root: &Path, n: usize, burst: bool) -> Result<Value, String> {
    let rel = root.join("target").join("release").join("ddalgak.exe");
    let gui = root.join("target").join("debug").join("ddalgak-gui.exe");
    let sb = Sandbox::new(&format!("h{n}"));
    sb.write(&rules_doc(n));
    let endpoint = format!("kim-bench-ddalgak-{n}-{}", std::process::id());
    let t = Instant::now();
    let mut child = sb
        .cmd(&gui)
        .args([
            "--test-host",
            "--test-max-seconds",
            "120",
            "--endpoint",
            &endpoint,
        ])
        .spawn()
        .map_err(|e| e.to_string())?;
    let ready = loop {
        let h = cli(&sb, &rel, &endpoint, &["health"]);
        if h["data"]["engine"]["hook"]["installed"] == true {
            break t.elapsed().as_secs_f64() * 1000.0;
        }
        if t.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            return Err(format!("the test host with {n} rules never got ready"));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    std::thread::sleep(Duration::from_secs(2));
    let h0 = cli(&sb, &rel, &endpoint, &["health"]);
    std::thread::sleep(Duration::from_secs(10));
    let h1 = cli(&sb, &rel, &endpoint, &["health"]);
    let f = |h: &Value, p: &str| h["data"].pointer(p).and_then(Value::as_f64).unwrap_or(0.0);
    let wake = |h: &Value| {
        f(h, "/counters/wakeups/hook_thread")
            + f(h, "/counters/wakeups/worker")
            + f(h, "/counters/wakeups/host")
    };
    // The CLI's own health requests wake the host: two per sample, not idle work.
    let wakeups = (wake(&h1) - wake(&h0) - 2.0).max(0.0);
    let pid = u32::try_from(h1["data"]["process"]["pid"].as_u64().unwrap_or(0)).unwrap_or(0);
    let (ws, private) = memory(pid);
    let mut out = json!({
        "ready_ms": r1(ready),
        "idle_cpu_ms_per_10s": f(&h1, "/process/cpu_ms") - f(&h0, "/process/cpu_ms"),
        "idle_wakeups_per_10s": wakeups,
        "working_set_mib": ws,
        "private_mib": private,
    });
    if burst {
        let ex = root
            .join("target")
            .join("release")
            .join("examples")
            .join("hook_burst.exe");
        let sent = no_window(&mut Command::new(ex))
            .args(["400", "2"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        std::thread::sleep(Duration::from_millis(300));
        let h2 = cli(&sb, &rel, &endpoint, &["health"]);
        out["burst_events"] = json!(sent.parse::<u64>().unwrap_or(0));
        out["hook_p99_us"] = h2["data"]["counters"]["callback_us"]["p99_at_most"].clone();
        out["hook_max_us"] = h2["data"]["counters"]["callback_us"]["max"].clone();
        // A profile switch of all rules (to the empty profile and back), saved and reloaded.
        let mut times = Vec::new();
        for to in ["other", "bench"] {
            let t = Instant::now();
            let v = sb
                .cmd(&rel)
                .args(["profile", "apply", to, "--endpoint", &endpoint, "--json"])
                .output();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            if v.map(|o| !o.status.success()).unwrap_or(true) {
                return Err(format!("profile apply {to} failed"));
            }
        }
        out["profile_switch_ms"] = json!(r1(times.iter().copied().fold(0.0, f64::max)));
    }
    let _ = cli(&sb, &rel, &endpoint, &["quit"]);
    let t = Instant::now();
    while child.try_wait().ok().flatten().is_none() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(out)
}

fn measure(root: &Path) -> Result<Value, String> {
    let rel = root.join("target").join("release");
    let size = |n: &str| {
        std::fs::metadata(rel.join(n))
            .map(|m| mib(m.len()))
            .unwrap_or(f64::INFINITY)
    };
    // Loading and validating 1000 rules (a cold CLI process, so an upper bound).
    let sb = Sandbox::new("load");
    sb.write(&rules_doc(1000));
    let mut load = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let o = sb
            .cmd(&rel.join("ddalgak.exe"))
            .args(["rule", "check", "--json"])
            .output()
            .map_err(|e| e.to_string())?;
        load.push(t.elapsed().as_secs_f64() * 1000.0);
        if !o.status.success() {
            return Err("rule check of 1000 rules failed".into());
        }
    }
    load.sort_by(f64::total_cmp);
    let mut hosts = serde_json::Map::new();
    for n in [0, 10, 100, 1000] {
        hosts.insert(n.to_string(), host_run(root, n, n == 1000)?);
    }
    let h = Value::Object(hosts);
    Ok(json!({
        "binary_mib": {"cli": size("ddalgak.exe"), "app": size("ddalgak-gui.exe"), "editor": size("ddalgak-editor.exe")},
        "check_1000_rules_ms": r1(load[load.len() / 2]),
        "hosts": h,
    }))
}

/// The budgets: (path, limit, requirement).
const BUDGETS: [(&str, f64, &str); 8] = [
    (
        "/hosts/100/idle_cpu_ms_per_10s",
        10.0,
        "DDALGAK-NFR-001 (0.1 % of one core, 100 rules)",
    ),
    (
        "/hosts/1000/idle_cpu_ms_per_10s",
        10.0,
        "DDALGAK-NFR-001 (and with 1000 rules)",
    ),
    (
        "/hosts/100/idle_wakeups_per_10s",
        2.0,
        "DDALGAK-NFR-001 (no periodic timers)",
    ),
    (
        "/hosts/1000/hook_p99_us",
        1000.0,
        "DDALGAK-NFR-002 (p99 < 1 ms)",
    ),
    (
        "/check_1000_rules_ms",
        300.0,
        "DDALGAK-NFR-004 (1000 rules loaded and checked, cold CLI)",
    ),
    (
        "/hosts/1000/ready_ms",
        1500.0,
        "DDALGAK-NFR-004 (1000 rules and the hook, debug test host start)",
    ),
    (
        "/hosts/1000/profile_switch_ms",
        1000.0,
        "DDALGAK-FR-030 (an atomic switch of 1000 rules, saved and reloaded)",
    ),
    ("/binary_mib/cli", 12.0, "release size"),
];

/// Measured and reported, but not a verdict: the only headless app is the debug test host, so
/// its memory is an upper bound for the release tray app (DDALGAK-NFR-003's 25 MB is checked on
/// the release build by hand, docs/DDALGAK_ACCEPTANCE.md).
const INFORMATIONAL: [(&str, f64, &str); 2] = [
    (
        "/hosts/100/working_set_mib",
        25.0,
        "DDALGAK-NFR-003 (release tray app; this is the debug test host)",
    ),
    (
        "/hosts/1000/working_set_mib",
        25.0,
        "DDALGAK-NFR-003 (release tray app; this is the debug test host)",
    ),
];

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("bench-ddalgak measures the Windows build; run it on Windows".into());
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for a in [
        &[
            "build",
            "--release",
            "-p",
            "ddalgak",
            "--bins",
            "--examples",
        ][..],
        &["build", "--release", "-p", "ddalgak-editor"][..],
        &["build", "-p", "ddalgak", "--bin", "ddalgak-gui"][..],
    ] {
        let s = Command::new(&cargo)
            .args(a)
            .current_dir(root)
            .status()
            .map_err(|e| e.to_string())?;
        if !s.success() {
            return Err(format!("cargo {} failed", a.join(" ")));
        }
    }
    let m = measure(root)?;
    println!("{}", serde_json::to_string_pretty(&m).unwrap_or_default());
    let mut over = Vec::new();
    for (path, limit, req) in BUDGETS {
        let v = m.pointer(path).and_then(Value::as_f64);
        let (ok, shown) = match v {
            Some(v) => (v <= limit, v.to_string()),
            // The hook burst is skipped on a locked desktop: not measured, not failed.
            None if path.contains("hook_p99") => (true, "not measured (locked desktop)".into()),
            None => (false, "missing".into()),
        };
        println!(
            "bench-ddalgak: {path} = {shown} (budget {limit}, {req}) {}",
            if ok { "ok" } else { "OVER" }
        );
        if !ok {
            over.push(path);
        }
    }
    for (path, limit, req) in INFORMATIONAL {
        let v = m.pointer(path).and_then(Value::as_f64).unwrap_or(f64::NAN);
        println!(
            "bench-ddalgak: {path} = {v} (release budget {limit}, {req}) informational, not verified here"
        );
    }
    if args.iter().any(|a| a == "--record") {
        let doc = json!({"schema": "kim.bench.ddalgak", "schema_version": 1,
            "machine": {"cpu": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),
                "processors": std::env::var("NUMBER_OF_PROCESSORS").unwrap_or_default()},
            "measured": m,
            "budgets": BUDGETS.iter().map(|(p, l, r)| json!({"path": p, "limit": l, "requirement": r})).collect::<Vec<_>>(),
            "informational": INFORMATIONAL.iter().map(|(p, l, r)| json!({"path": p, "release_limit": l, "requirement": r, "verified": false})).collect::<Vec<_>>()});
        let p = root.join("docs").join("perf").join("ddalgak-baseline.json");
        std::fs::write(
            &p,
            serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n",
        )
        .map_err(|e| e.to_string())?;
        println!("bench-ddalgak: recorded {}", p.display());
    }
    if args.iter().any(|a| a == "--check") && !over.is_empty() {
        return Err(format!("over budget: {over:?}"));
    }
    Ok(())
}

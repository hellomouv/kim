//! `cargo xtask bench` (Step 39, docs/PERFORMANCE.md): measures the release `subak` and `kim`
//! on this machine — start and command latency, memory per Terminal and pane, idle CPU, disk and
//! network, a high output rate, a Unicode round trip, a create/kill soak for leaks, and malformed
//! MCP input — and compares the result with the recorded budgets.
//!
//! - `cargo xtask bench` measures and prints; writes `target/bench/latest.json`.
//! - `--check` also fails when a measurement exceeds its budget in `docs/perf/baseline.json`.
//! - `--record` writes the measurements and their budgets as the new baseline (review the diff).
//!
//! Everything runs against a private endpoint and a sandboxed `APPDATA`/`LOCALAPPDATA`; no
//! window opens; nothing touches the network. Never part of `cargo xtask ci` (timings depend
//! on the machine); run it before a release and after performance-relevant changes.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Bench {
    subak: PathBuf,
    kim: PathBuf,
    ep: String,
    root: PathBuf,
}

fn no_window(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

impl Bench {
    fn cmd(&self, exe: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(exe);
        c.args(args)
            .env("APPDATA", self.root.join("Roaming"))
            .env("LOCALAPPDATA", self.root.join("Local"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        no_window(&mut c);
        c
    }

    fn subak(&self, args: &[&str]) -> Result<Value, String> {
        let mut a = vec!["--json", "--endpoint", self.ep.as_str()];
        a.extend_from_slice(args);
        let out: Output = self
            .cmd(&self.subak, &a)
            .output()
            .map_err(|e| format!("subak: {e}"))?;
        serde_json::from_slice(&out.stdout).map_err(|e| format!("subak {args:?}: {e}"))
    }

    fn ok(&self, args: &[&str]) -> Result<Value, String> {
        let v = self.subak(args)?;
        if v["ok"] == true {
            Ok(v["data"].clone())
        } else {
            Err(format!("subak {args:?}: {}", v["error"]))
        }
    }

    fn timed(&self, args: &[&str]) -> Result<f64, String> {
        let t = Instant::now();
        self.ok(args)?;
        Ok(t.elapsed().as_secs_f64() * 1000.0)
    }

    fn stats(&self) -> Result<Value, String> {
        self.ok(&["host", "stats"])
    }

    fn screen(&self, t: &str) -> String {
        self.ok(&["capture", t])
            .map(|d| {
                d["lines"]
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    fn wait_screen(&self, t: &str, want: &str, limit: Duration) -> Option<f64> {
        let t0 = Instant::now();
        while t0.elapsed() < limit {
            if self.screen(t).contains(want) {
                return Some(t0.elapsed().as_secs_f64() * 1000.0);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    fn stop(&self) {
        let _ = self.subak(&["host", "stop", "--kill-terminals", "--yes"]);
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return 0.0;
    }
    v[v.len() / 2]
}

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return 0.0;
    }
    v[((v.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(v.len() - 1)]
}

fn mem(s: &Value) -> (f64, f64) {
    let m = &s["host"]["memory"];
    (
        m["private_bytes"].as_f64().unwrap_or(0.0) / 1_048_576.0,
        m["working_set_bytes"].as_f64().unwrap_or(0.0) / 1_048_576.0,
    )
}

fn cpu(s: &Value) -> f64 {
    s["host"]["cpu"]["kernel_ms"].as_f64().unwrap_or(0.0)
        + s["host"]["cpu"]["user_ms"].as_f64().unwrap_or(0.0)
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// TCP and UDP endpoints owned by `pid` (`netstat -ano`).
fn sockets_of(pid: u64) -> usize {
    let mut c = Command::new("netstat");
    c.args(["-ano"]).stdin(Stdio::null()).stderr(Stdio::null());
    no_window(&mut c);
    let Ok(out) = c.output() else {
        return usize::MAX;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.split_whitespace().last() == Some(pid.to_string().as_str()))
        .count()
}

fn measure(b: &Bench) -> Result<Value, String> {
    let mut m = serde_json::Map::new();
    let put = |m: &mut serde_json::Map<String, Value>, k: &str, v: Value| {
        eprintln!("bench: {k} = {v}");
        m.insert(k.to_string(), v);
    };
    let state = b.root.join("state");
    let state_s = state.display().to_string();

    // 1. Host start (cold, no Terminal), five times.
    let mut starts = Vec::new();
    for _ in 0..5 {
        b.stop();
        starts.push(b.timed(&["host", "start", "--no-shell", "--state-dir", &state_s])?);
    }
    put(
        &mut m,
        "host_start_ms_median",
        json!(r1(median(starts.clone()))),
    );
    put(&mut m, "host_start_ms_p95", json!(r1(p95(starts))));
    let s0 = b.stats()?;
    let (p0, w0) = mem(&s0);
    put(&mut m, "host_idle_private_mib", json!(r1(p0)));
    put(&mut m, "host_idle_working_set_mib", json!(r1(w0)));

    // 2. Terminals: create ten, time each; memory per Terminal; first prompt.
    let mut creates = Vec::new();
    for i in 1..=10 {
        creates.push(b.timed(&["new", &format!("t{i}")])?);
        if i == 1 {
            let first = b
                .wait_screen("t1", ">", Duration::from_secs(20))
                .ok_or("t1 showed no prompt")?;
            put(&mut m, "first_prompt_ms", json!(r1(first)));
        }
    }
    put(
        &mut m,
        "new_terminal_ms_median",
        json!(r1(median(creates.clone()))),
    );
    put(&mut m, "new_terminal_ms_p95", json!(r1(p95(creates))));
    std::thread::sleep(Duration::from_secs(2));
    let s10 = b.stats()?;
    let (p10, w10) = mem(&s10);
    put(
        &mut m,
        "host_private_mib_per_terminal",
        json!(r1((p10 - p0) / 10.0)),
    );
    put(&mut m, "host_working_set_mib_10_terminals", json!(r1(w10)));

    // 3. A CLI round trip (process start + IPC + reply).
    let mut caps = Vec::new();
    for _ in 0..20 {
        caps.push(b.timed(&["capture", "t1"])?);
    }
    put(
        &mut m,
        "cli_capture_ms_median",
        json!(r1(median(caps.clone()))),
    );
    put(&mut m, "cli_capture_ms_p95", json!(r1(p95(caps))));

    // 4. Panes: one split in each of nine Terminals (nine more panes).
    let before = mem(&b.stats()?).0;
    for i in 2..=10 {
        b.ok(&["pane", "split", &format!("t{i}"), "--horizontal"])?;
    }
    std::thread::sleep(Duration::from_secs(2));
    put(
        &mut m,
        "host_private_mib_per_pane",
        json!(r1((mem(&b.stats()?).0 - before) / 9.0)),
    );

    // 5. Idle: CPU, state writes and sockets over 10 s with 10 Terminals and 10 panes.
    std::thread::sleep(Duration::from_secs(2));
    let pid = b.stats()?["pid"].as_u64().unwrap_or(0);
    let state_file = state.join("state.json");
    let mtime = || {
        std::fs::metadata(&state_file)
            .and_then(|m| m.modified())
            .ok()
    };
    let (c0, t0) = (cpu(&b.stats()?), mtime());
    std::thread::sleep(Duration::from_secs(10));
    let (c1, t1) = (cpu(&b.stats()?), mtime());
    put(&mut m, "idle_host_cpu_ms_per_10s", json!(r1(c1 - c0)));
    put(&mut m, "idle_state_writes_10s", json!(u8::from(t0 != t1)));
    put(&mut m, "host_sockets", json!(sockets_of(pid)));

    // 6. A high output rate: 100000 lines in one pane.
    let c0 = cpu(&b.stats()?);
    let t = Instant::now();
    b.ok(&[
        "command",
        "--target",
        "t3",
        "send-keys -l 'for /l %i in (1,1,100000) do @echo line-%i'",
    ])?;
    b.ok(&["command", "--target", "t3", "send-keys Enter"])?;
    b.wait_screen("t3", "line-100000", Duration::from_secs(300))
        .ok_or("100000 lines never finished")?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    put(&mut m, "output_100k_lines_ms", json!(r1(ms)));
    put(
        &mut m,
        "output_100k_lines_host_cpu_ms",
        json!(r1(cpu(&b.stats()?) - c0)),
    );

    // 7. Unicode comes back as typed.
    let text = "uni 한글 日本語 Ελληνικά ✓ 👍";
    b.ok(&[
        "command",
        "--target",
        "t4",
        &format!("send-keys -l 'echo {text}'"),
    ])?;
    b.ok(&["command", "--target", "t4", "send-keys Enter"])?;
    let ok = b
        .wait_screen("t4", &format!("\n{text}"), Duration::from_secs(10))
        .is_some()
        || b.screen("t4").matches(text).count() >= 2;
    put(&mut m, "unicode_round_trip", json!(ok));

    // 8. Soak: 50 create/kill cycles and 200 captures; handles and memory before and after.
    std::thread::sleep(Duration::from_secs(1));
    let sb = b.stats()?;
    for i in 0..50 {
        let n = format!("soak{i}");
        b.ok(&["new", &n])?;
        b.ok(&["rm", &n, "--force", "--yes"])?;
    }
    for _ in 0..200 {
        b.ok(&["capture", "t5"])?;
    }
    std::thread::sleep(Duration::from_secs(3));
    let sa = b.stats()?;
    put(
        &mut m,
        "soak_handle_delta",
        json!(
            sa["host"]["handle_count"].as_i64().unwrap_or(0)
                - sb["host"]["handle_count"].as_i64().unwrap_or(0)
        ),
    );
    put(
        &mut m,
        "soak_private_mib_delta",
        json!(r1(mem(&sa).0 - mem(&sb).0)),
    );

    // 9. Malformed MCP input: every line answered, the server alive, then a valid call.
    let mut c = b.cmd(&b.subak, &["mcp", "serve", "--endpoint", &b.ep]);
    let mut child = c.stdin(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let mut tx = child.stdin.take().ok_or("stdin")?;
    let rx = child.stdout.take().ok_or("stdout")?;
    let reader = std::thread::spawn(move || {
        use std::io::BufRead;
        std::io::BufReader::new(rx)
            .lines()
            .map_while(Result::ok)
            .collect::<Vec<String>>()
    });
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut sent = 0;
    {
        use std::io::Write;
        for i in 0..300u32 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let junk = match i % 4 {
                0 => format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":{i},\"method\":\"tools/call\",\"params\":{{\"name\":\"read_screen\",\"arguments\":{{\"target\":{seed}}}}}}}"
                ),
                1 => format!("{{\"jsonrpc\":\"2.0\",\"id\":{i},\"method\":\"{seed:x}\"}}"),
                2 => format!("{{\"id\":{i},\"jsonrpc\":1,\"method\":[]}}"),
                _ => format!("{seed:x}{{]\""),
            };
            if writeln!(tx, "{junk}").is_err() {
                break;
            }
            sent += 1;
        }
        let _ = writeln!(
            tx,
            "{{\"jsonrpc\":\"2.0\",\"id\":9999,\"method\":\"ping\"}}"
        );
    }
    drop(tx);
    let lines = reader.join().map_err(|_| "reader")?;
    let _ = child.wait();
    let all_json = lines
        .iter()
        .all(|l| serde_json::from_str::<Value>(l).is_ok());
    let pong = lines
        .iter()
        .any(|l| l.contains("\"id\":9999") && l.contains("\"result\""));
    put(
        &mut m,
        "mcp_malformed_answered",
        json!(lines.len() >= sent && all_json && pong),
    );

    // 10. Kim: a status call (reads files only).
    let mut ks = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let out = b
            .cmd(&b.kim, &["status", "--json"])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err("kim status failed".into());
        }
        ks.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    put(&mut m, "kim_status_ms_median", json!(r1(median(ks))));
    b.stop();
    Ok(Value::Object(m))
}

/// The budget of one measurement: what a later run may reach before it is a regression.
fn budget(key: &str, v: &Value) -> Value {
    if let Some(b) = v.as_bool() {
        return json!(b);
    }
    let x = v.as_f64().unwrap_or(0.0);
    let limit = match key {
        // Must stay zero.
        "host_sockets" | "idle_state_writes_10s" => 0.0,
        // Leaks: a fixed allowance, whatever the baseline.
        // Windows 10's inbox ConPTY keeps one exited conhost handle per pane (50 cycles = 50;
        // `sessions_hold_no_handle_beyond_the_one_the_inbox_conpty_keeps`); anything more is ours.
        "soak_handle_delta" => 50.0 + 16.0,
        "soak_private_mib_delta" => 8.0,
        k if k.ends_with("_ms") || k.contains("_ms_") => (x * 2.0).max(x + 50.0),
        k if k.contains("mib") => (x * 1.5).max(x + 2.0),
        _ => x * 2.0,
    };
    json!(r1(limit))
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("bench measures the Windows Host; run it on Windows".into());
    }
    let check = args.iter().any(|a| a == "--check");
    let record = args.iter().any(|a| a == "--record");
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "-p", "subak", "-p", "kim"])
        .current_dir(root)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("cargo build --release failed".into());
    }
    let rel = root.join("target").join("release");
    let sandbox = std::env::temp_dir().join(format!("kim-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(sandbox.join("Roaming")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(sandbox.join("Local")).map_err(|e| e.to_string())?;
    let b = Bench {
        subak: rel.join("subak.exe"),
        kim: rel.join("kim.exe"),
        ep: format!("kim-bench-{}", std::process::id()),
        root: sandbox.clone(),
    };
    let measured = measure(&b);
    b.stop();
    let _ = std::fs::remove_dir_all(&sandbox);
    let measured = measured?;
    let machine = json!({
        "os": std::env::var("OS").unwrap_or_default(),
        "processors": std::env::var("NUMBER_OF_PROCESSORS").unwrap_or_default(),
        "cpu": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),
        "commit": Command::new("git").args(["rev-parse", "--short", "HEAD"]).current_dir(root).output().ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default(),
    });
    let doc = json!({"machine": machine, "measured": measured});
    let out_dir = root.join("target").join("bench");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        out_dir.join("latest.json"),
        serde_json::to_vec_pretty(&doc).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let baseline_path = root.join("docs").join("perf").join("baseline.json");
    if record {
        let budgets: serde_json::Map<String, Value> = measured
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), budget(k, v)))
            .collect();
        let rec = json!({"machine": doc["machine"], "measured": measured, "budget": budgets});
        std::fs::create_dir_all(baseline_path.parent().unwrap_or(root))
            .map_err(|e| e.to_string())?;
        std::fs::write(
            &baseline_path,
            serde_json::to_string_pretty(&rec).unwrap_or_default() + "\n",
        )
        .map_err(|e| e.to_string())?;
        println!("bench: recorded {}", baseline_path.display());
    }
    if check || !record {
        let base: Value = std::fs::read(&baseline_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let mut over = Vec::new();
        println!("{:<38} {:>12} {:>12}", "measurement", "now", "budget");
        for (k, v) in measured.as_object().into_iter().flatten() {
            let lim = &base["budget"][k];
            let bad = match (v, lim) {
                (Value::Bool(b), Value::Bool(want)) => b != want,
                (n, l) if l.is_number() => {
                    n.as_f64().unwrap_or(0.0) > l.as_f64().unwrap_or(f64::MAX)
                }
                _ => false,
            };
            println!(
                "{:<38} {:>12} {:>12}{}",
                k,
                v.to_string(),
                lim.to_string(),
                if bad { "  OVER" } else { "" }
            );
            if bad {
                over.push(k.clone());
            }
        }
        if check && !over.is_empty() {
            return Err(format!("bench: over budget: {}", over.join(", ")));
        }
    }
    Ok(())
}

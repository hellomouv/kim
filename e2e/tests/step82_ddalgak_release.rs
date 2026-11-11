//! E2E — Step 82: Ddalgak's release safety (docs/DDALGAK_ACCEPTANCE.md rows 1, 8, 14, 15;
//! docs/SECURITY.md §10). Requirements: `DDALGAK-NFR-001`, `DDALGAK-SEC-003`, `DDALGAK-SEC-005`,
//! `KIM-SEC-001`, `KIM-FR-012`.
//!
//! A first run in an empty profile (release `ddalgak`, the debug test host: no tray icon, a hook
//! that never consumes a key): no rules file, nothing written by `status`, `rule check` or
//! `capabilities`; sign-in start is not changed by starting the app; the engine starts **off**
//! (`default`) with no hook until it is turned on; turned on it installs the hook, then `release`
//! and `disable` (the emergency's CLI twins) leave no hook and nothing held; the app quits cleanly
//! and a second start is not an unclean exit. The release CLI stays within its size budget.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`, own pipe endpoint; no key is sent; the
//! sign-in setting is only read; no window or tray icon appears.
#![cfg(windows)]

use kim_e2e::{TempDir, bin, bin_debug, require_e2e};
use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct Host(Child);

impl Drop for Host {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn cmd(exe: &Path, sb: &TempDir) -> Command {
    let mut c = Command::new(exe);
    c.env("APPDATA", sb.path().join("Roaming"))
        .env("LOCALAPPDATA", sb.path().join("Local"))
        .creation_flags(CREATE_NO_WINDOW);
    c
}

fn json(sb: &TempDir, endpoint: &str, args: &[&str]) -> Value {
    let mut c = cmd(&bin("ddalgak"), sb);
    c.args(args).arg("--json");
    // Only the commands that talk to the app take an endpoint.
    if matches!(
        args.first(),
        Some(&("status" | "health" | "engine" | "release" | "disable" | "enable" | "quit"))
    ) {
        c.args(["--endpoint", endpoint]);
    }
    let out = c.output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or(Value::Null)
}

fn files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |d| {
        d.flatten()
            .map(|e| {
                if e.path().is_dir() {
                    files(&e.path())
                } else {
                    1
                }
            })
            .sum()
    })
}

fn start(sb: &TempDir, endpoint: &str) -> Host {
    let mut c = cmd(&bin_debug("ddalgak-gui"), sb);
    c.args([
        "--test-host",
        "--test-max-seconds",
        "60",
        "--endpoint",
        endpoint,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let mut h = Host(c.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(20);
    while json(sb, endpoint, &["status"])["data"]["app"]["running"] != true {
        assert!(h.0.try_wait().unwrap().is_none(), "the app exited early");
        assert!(Instant::now() < until, "the app never answered");
        std::thread::sleep(Duration::from_millis(100));
    }
    h
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step82_a_first_run_is_safe_off_by_default_and_recoverable() {
    require_e2e();
    let sb = TempDir::new("step82");
    std::fs::create_dir_all(sb.path().join("Roaming")).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let ep = format!("kim-e2e-step82-{}", std::process::id());
    let size = std::fs::metadata(bin("ddalgak")).unwrap().len();
    assert!(size < 12 * 1024 * 1024, "ddalgak.exe is {size} bytes");

    // Inspecting writes nothing.
    for a in [
        &["status"][..],
        &["rule", "check"][..],
        &["capabilities"][..],
        &["mcp", "tools"][..],
    ] {
        let v = json(&sb, &ep, a);
        assert_eq!(v["ok"], true, "{a:?}: {v}");
    }
    assert_eq!(
        files(&sb.path().join("Roaming")),
        0,
        "no file written by inspection"
    );
    let startup = json(&sb, &ep, &["startup", "status"])["data"].clone();

    // A first start: engine off, no hook; sign-in start untouched.
    let host = start(&sb, &ep);
    let st = json(&sb, &ep, &["status"])["data"].clone();
    assert_eq!(st["engine"]["state"], "off", "{st:#}");
    if !ddalgak::win::is_elevated() {
        assert_eq!(st["engine"]["off_reason"], "default", "{st:#}");
    }
    assert_eq!(
        json(&sb, &ep, &["health"])["data"]["engine"]["hook"]["installed"],
        false
    );
    assert_eq!(
        json(&sb, &ep, &["startup", "status"])["data"],
        startup,
        "starting never changes sign-in start"
    );

    if !ddalgak::win::is_elevated() {
        // On, then the emergency's CLI twins: nothing held, then no hook.
        let v = json(&sb, &ep, &["engine", "start"]);
        assert_eq!(v["data"]["engine"]["state"], "running", "{v}");
        assert_eq!(
            json(&sb, &ep, &["health"])["data"]["engine"]["hook"]["installed"],
            true
        );
        let v = json(&sb, &ep, &["release"]);
        assert_eq!(v["ok"], true, "{v}");
        let v = json(&sb, &ep, &["disable"]);
        assert_eq!(v["data"]["engine"]["state"], "off", "{v}");
        assert_eq!(
            json(&sb, &ep, &["health"])["data"]["engine"]["hook"]["installed"],
            false
        );
    }
    assert_eq!(json(&sb, &ep, &["quit"])["ok"], true);
    let mut host = host;
    let t = Instant::now();
    while host.0.try_wait().unwrap().is_none() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "the app did not quit"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // A clean quit: the next start is not an unclean exit.
    let _host = start(&sb, &ep);
    let st = json(&sb, &ep, &["status"])["data"].clone();
    assert_ne!(st["start"]["off_reason"], "unclean_exit", "{st:#}");
    json(&sb, &ep, &["quit"]);
}

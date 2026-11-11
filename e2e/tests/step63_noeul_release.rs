//! E2E — Step 63: the release Noeul meets its resource promises on a machine in use
//! (docs/PERFORMANCE.md §5, docs/SECURITY.md §9). Requirements: NOEUL-NFR-001, NOEUL-NFR-002,
//! NOEUL-NFR-006, NOEUL-SEC-005.
//!
//! The release `noeul.exe`: at most 12 MiB; `noeul run --no-tray --no-hotkeys` in a private
//! profile (no window, no tray icon, no hotkey) answers quickly, idles without CPU, writes no file
//! anywhere in the profile, and quits cleanly. Looser limits than `cargo xtask bench-noeul` (a
//! shared CI machine); the bench holds the exact budgets. Windows only.
#![cfg(windows)]

use kim_e2e::{TempDir, bin};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn cpu_ms(pid: u32) -> f64 {
    let o = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(Get-Process -Id {pid}).TotalProcessorTime.TotalMilliseconds"),
        ])
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .trim()
        .replace(',', ".")
        .parse()
        .unwrap_or(f64::NAN)
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn the_release_noeul_is_small_quick_quiet_and_writes_nothing() {
    let exe = bin("noeul");
    let size = std::fs::metadata(&exe).unwrap().len();
    assert!(
        size <= 12 * 1024 * 1024,
        "noeul.exe is {size} bytes (NOEUL-NFR-006)"
    );
    let sb = TempDir::new("noeul63");
    let p = sb.path();
    std::fs::create_dir_all(p.join("Roaming")).unwrap();
    std::fs::create_dir_all(p.join("Local")).unwrap();
    let ep = format!("noeul-e2e63-{}", std::process::id());
    let cmd = |args: &[&str]| {
        let mut c = Command::new(&exe);
        c.args(args)
            .args(["--endpoint", &ep])
            .env("APPDATA", p.join("Roaming"))
            .env("LOCALAPPDATA", p.join("Local"))
            .env("KIM_NO_INPUT", "1")
            .env_remove("NOEUL_DEV_SYNTHETIC_DESKTOP")
            .stdin(Stdio::null())
            .creation_flags(0x0800_0000);
        c
    };
    let t = Instant::now();
    let mut child = cmd(&["run", "--no-tray", "--no-hotkeys"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    while !cmd(&["health", "--json"])
        .output()
        .is_ok_and(|o| o.status.success())
    {
        assert!(t.elapsed() < Duration::from_secs(5), "never healthy");
        std::thread::sleep(Duration::from_millis(10));
    }
    let ready = t.elapsed();
    assert!(ready < Duration::from_secs(2), "ready after {ready:?}");
    std::thread::sleep(Duration::from_secs(1));
    let c0 = cpu_ms(child.id());
    std::thread::sleep(Duration::from_secs(5));
    let idle = cpu_ms(child.id()) - c0;
    assert!(idle <= 50.0, "idle CPU {idle} ms over 5 s (NOEUL-NFR-001)");
    let (code, _) = (
        cmd(&["quit", "--yes", "--json"]).status().unwrap().code(),
        (),
    );
    assert_eq!(code, Some(0));
    let _ = child.wait();
    let written: Vec<_> = files(&p.join("Local"))
        .into_iter()
        .chain(files(&p.join("Roaming")))
        .collect();
    assert!(
        written.is_empty(),
        "nothing written while idle: {written:?} (NOEUL-SEC-005)"
    );
    println!("EVIDENCE release noeul: {size} bytes, ready in {ready:?}, idle CPU {idle} ms / 5 s");
}

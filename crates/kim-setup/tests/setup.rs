//! Step 43 — `Kim-Setup.exe` headless (ADR 0072). `KIM-FR-060`, `KIM-SEC-001`, `KIM-SEC-003`.
//!
//! The real installer program with test payloads appended, in a sandboxed `LOCALAPPDATA` and
//! `APPDATA` and a test registry key (never the real Apps & Features entry): a fresh install of
//! only Kim (files, record, shortcut, entry, no network), a second run refused, a repair, an
//! upgrade keeping the previous version, a downgrade refused, a damaged installer refused with
//! nothing changed, an uninstall refused while Kim runs, then done keeping the settings.
#![cfg(windows)]

use serde_json::Value;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const NO_WINDOW: u32 = 0x0800_0000;

fn payload(version: &str) -> Vec<u8> {
    let stub = std::fs::read(env!("CARGO_BIN_EXE_kim-setup")).unwrap();
    // A program that keeps running, for the "Kim is running" case.
    let ping = std::fs::read(r"C:\Windows\System32\PING.EXE").unwrap();
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for (n, b) in [
        // `kim.exe --version` must succeed: the installer program answers it.
        ("kim.exe", stub),
        ("kim-manager.exe", ping),
        (
            "capabilities.json",
            format!("{{\"tool\":\"kim\",\"version\":\"{version}\"}}").into_bytes(),
        ),
        ("LICENSE-MIT", b"MIT".to_vec()),
    ] {
        z.start_file(n, o).unwrap();
        z.write_all(&b).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn installer(dir: &Path, version: &str) -> PathBuf {
    let stub = std::fs::read(env!("CARGO_BIN_EXE_kim-setup")).unwrap();
    let p = dir.join(format!("Kim-Setup-{version}.exe"));
    std::fs::write(&p, kim_setup::assemble(&stub, &payload(version))).unwrap();
    p
}

struct Sb {
    root: PathBuf,
    key: String,
}

impl Sb {
    fn run(&self, exe: &Path, args: &[&str]) -> (i32, Value) {
        let out = Command::new(exe)
            .args(args)
            .args(["--quiet", "--json"])
            .env("LOCALAPPDATA", self.root.join("Local"))
            .env("APPDATA", self.root.join("Roaming"))
            .env("KIM_SETUP_TEST_REGISTRY_KEY", &self.key)
            .creation_flags(NO_WINDOW)
            .output()
            .unwrap();
        let v: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|_| panic!("{args:?}: {}", String::from_utf8_lossy(&out.stdout)));
        (out.status.code().unwrap_or(-1), v)
    }

    fn tool(&self) -> PathBuf {
        self.root
            .join("Local")
            .join("Programs")
            .join("mouv")
            .join("kim")
    }

    fn reg_version(&self) -> Option<String> {
        let out = Command::new("reg")
            .args([
                "query",
                &format!(r"HKCU\{}", self.key),
                "/v",
                "DisplayVersion",
            ])
            .creation_flags(NO_WINDOW)
            .output()
            .unwrap();
        out.status.success().then(|| {
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_string()
        })
    }
}

impl Drop for Sb {
    fn drop(&mut self) {
        let _ = Command::new("reg")
            .args(["delete", &format!(r"HKCU\{}", self.key), "/f"])
            .creation_flags(NO_WINDOW)
            .output();
        // The parent `Software\mouv-tests` exists only for tests.
        let _ = Command::new("reg")
            .args(["delete", r"HKCU\Software\mouv-tests", "/f"])
            .creation_flags(NO_WINDOW)
            .output();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn kim_setup_installs_only_kim_and_round_trips_safely() {
    let id = std::process::id();
    let sb = Sb {
        root: std::env::temp_dir().join(format!("kim-setup-t-{id}")),
        key: format!(r"Software\mouv-tests\kim-setup-{id}"),
    };
    let _ = std::fs::remove_dir_all(&sb.root);
    std::fs::create_dir_all(sb.root.join("Local")).unwrap();
    std::fs::create_dir_all(sb.root.join("Roaming")).unwrap();
    let dl = sb.root.join("Downloads");
    std::fs::create_dir_all(&dl).unwrap();
    let (s1, s2, s0) = (
        installer(&dl, "0.1.0"),
        installer(&dl, "0.2.0"),
        installer(&dl, "0.0.9"),
    );

    // A dry run changes nothing.
    let (code, v) = sb.run(&s1, &["--dry-run"]);
    assert_eq!(code, 0, "{v}");
    assert!(!sb.tool().exists());

    // Fresh install: only Kim, in Kim's own layout, with a shortcut and an Apps & Features entry.
    let (code, v) = sb.run(&s1, &[]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["action"], "install");
    let programs = sb.root.join("Local").join("Programs").join("mouv");
    let tools: Vec<String> = std::fs::read_dir(&programs)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(tools, ["kim"], "only Kim is installed");
    let rec: Value =
        serde_json::from_slice(&std::fs::read(sb.tool().join("current.json")).unwrap()).unwrap();
    assert_eq!(
        (
            rec["schema"].as_str(),
            rec["tool"].as_str(),
            rec["version"].as_str()
        ),
        (Some("kim.install"), Some("kim"), Some("0.1.0"))
    );
    assert!(sb.tool().join("versions/0.1.0/kim.exe").is_file());
    assert!(
        sb.tool().join("kim-setup.exe").is_file(),
        "kept for uninstall and repair"
    );
    let lnk = sb
        .root
        .join(r"Roaming\Microsoft\Windows\Start Menu\Programs\Kim Manager.lnk");
    assert!(
        std::fs::metadata(&lnk).is_ok_and(|m| m.len() > 0),
        "the Start Menu shortcut"
    );
    assert_eq!(sb.reg_version().as_deref(), Some("0.1.0"));
    assert!(
        !sb.tool()
            .join(".staging")
            .read_dir()
            .is_ok_and(|mut d| d.next().is_some()),
        "no staging left"
    );

    // Again: refused; with --repair: restored.
    let (code, v) = sb.run(&s1, &[]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("already_installed")),
        "{v}"
    );
    std::fs::remove_file(sb.tool().join("versions/0.1.0/kim-manager.exe")).unwrap();
    let (code, v) = sb.run(&s1, &["--repair"]);
    assert_eq!(
        (code, v["data"]["action"].as_str()),
        (0, Some("repair")),
        "{v}"
    );
    assert!(sb.tool().join("versions/0.1.0/kim-manager.exe").is_file());

    // Upgrade keeps the previous version for `kim rollback`; an older installer is refused.
    let (code, v) = sb.run(&s2, &[]);
    assert_eq!(
        (code, v["data"]["action"].as_str()),
        (0, Some("upgrade")),
        "{v}"
    );
    let rec: Value =
        serde_json::from_slice(&std::fs::read(sb.tool().join("current.json")).unwrap()).unwrap();
    assert_eq!(
        (rec["version"].as_str(), rec["previous_version"].as_str()),
        (Some("0.2.0"), Some("0.1.0"))
    );
    assert!(sb.tool().join("versions/0.1.0").is_dir());
    assert_eq!(sb.reg_version().as_deref(), Some("0.2.0"));
    let (code, v) = sb.run(&s0, &[]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("newer_installed")),
        "{v}"
    );

    // A damaged installer: refused before anything is touched.
    let mut bytes = std::fs::read(&s2).unwrap();
    let n = bytes.len();
    bytes[n - 4096] ^= 0x55;
    let bad = dl.join("Kim-Setup-damaged.exe");
    std::fs::write(&bad, bytes).unwrap();
    let before = std::fs::read(sb.tool().join("current.json")).unwrap();
    let (code, v) = sb.run(&bad, &["--repair"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (4, Some("integrity_failed")),
        "{v}"
    );
    assert_eq!(
        std::fs::read(sb.tool().join("current.json")).unwrap(),
        before
    );

    // Uninstall: refused while a Kim program runs (never stopped), then done; settings kept.
    let data = sb.root.join(r"Roaming\mouv\kim");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("settings.json"), "{}").unwrap();
    let mut running = Command::new(sb.tool().join("versions/0.2.0/kim-manager.exe"))
        .args(["-n", "60", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .creation_flags(NO_WINDOW)
        .spawn()
        .unwrap();
    let uninstaller = sb.tool().join("kim-setup.exe");
    let (code, v) = sb.run(&uninstaller, &["--uninstall", "--yes"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("tool_running")),
        "{v}"
    );
    assert!(sb.tool().join("current.json").is_file());
    let _ = running.kill();
    let _ = running.wait();
    let (code, v) = sb.run(&uninstaller, &["--uninstall"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("confirmation_required")),
        "{v}"
    );
    let (code, v) = sb.run(&uninstaller, &["--uninstall", "--yes"]);
    assert_eq!(code, 0, "{v}");
    assert!(!sb.tool().exists(), "{v}");
    assert!(!lnk.exists());
    assert_eq!(sb.reg_version(), None);
    assert!(data.join("settings.json").is_file(), "settings kept");
}

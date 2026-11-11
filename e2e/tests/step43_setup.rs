//! E2E — Step 43 (Windows): the one-step installer with the real Kim programs (ADR 0072).
//! `KIM-FR-060`, `KIM-SEC-001`.
//!
//! The release `kim.exe` and `kim-manager.exe`, zipped as `cargo xtask package kim` lays them
//! out, appended to the installer program (its debug build, the only one that honors a test
//! registry key, so the real Apps & Features entry is never touched), run headless (`--quiet`)
//! in a sandbox: only Kim is installed, the installed `kim` works and lists no other tool, the
//! installer needs no network, and uninstall removes Kim while keeping the settings.
#![cfg(windows)]

use kim_e2e::{TempDir, bin, bin_debug};
use serde_json::Value;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

const NO_WINDOW: u32 = 0x0800_0000;

fn run(exe: &Path, sb: &Path, key: &str, args: &[&str]) -> (i32, Value) {
    let out = Command::new(exe)
        .args(args)
        .env("LOCALAPPDATA", sb.join("Local"))
        .env("APPDATA", sb.join("Roaming"))
        .env("KIM_SETUP_TEST_REGISTRY_KEY", key)
        .creation_flags(NO_WINDOW)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn one_installer_installs_only_kim_and_kim_works() {
    let tmp = TempDir::new("setup43");
    let sb = tmp.path();
    std::fs::create_dir_all(sb.join("Local")).unwrap();
    std::fs::create_dir_all(sb.join("Roaming")).unwrap();
    let key = format!(r"Software\mouv-tests\e2e43-{}", std::process::id());
    let kim = bin("kim");
    let caps = Command::new(&kim)
        .args(["capabilities", "--json"])
        .creation_flags(NO_WINDOW)
        .output()
        .unwrap();
    let caps: Value = serde_json::from_slice(&caps.stdout).unwrap();
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for (n, b) in [
        ("kim.exe", std::fs::read(&kim).unwrap()),
        (
            "kim-manager.exe",
            std::fs::read(kim.with_file_name("kim-manager.exe")).unwrap(),
        ),
        (
            "capabilities.json",
            serde_json::to_vec_pretty(&caps["data"]).unwrap(),
        ),
    ] {
        z.start_file(n, o).unwrap();
        z.write_all(&b).unwrap();
    }
    let payload = z.finish().unwrap().into_inner();
    let setup = sb.join("Kim-Setup.exe");
    std::fs::write(
        &setup,
        kim_setup::assemble(&std::fs::read(bin_debug("kim-setup")).unwrap(), &payload),
    )
    .unwrap();

    let (code, v) = run(&setup, sb, &key, &["--quiet", "--json"]);
    assert_eq!(code, 0, "{v}");
    let version = caps["data"]["version"].as_str().unwrap();
    assert_eq!(v["data"]["version"], version);
    let programs = sb.join(r"Local\Programs\mouv");
    let tools: Vec<String> = std::fs::read_dir(&programs)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(tools, ["kim"], "only Kim");
    let installed = programs.join(format!(r"kim\versions\{version}\kim.exe"));
    let (code, v) = run(&installed, sb, &key, &["tools", "list", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        v["data"]["tools"].as_array().unwrap().len(),
        1,
        "no other tool: {v}"
    );
    assert!(
        sb.join(r"Roaming\Microsoft\Windows\Start Menu\Programs\Kim Manager.lnk")
            .is_file()
    );

    std::fs::create_dir_all(sb.join(r"Roaming\mouv\kim")).unwrap();
    std::fs::write(sb.join(r"Roaming\mouv\kim\settings.json"), "{}").unwrap();
    let (code, v) = run(
        &programs.join(r"kim\kim-setup.exe"),
        sb,
        &key,
        &["--uninstall", "--quiet", "--yes", "--json"],
    );
    assert_eq!(code, 0, "{v}");
    assert!(!programs.join("kim").exists());
    assert!(sb.join(r"Roaming\mouv\kim\settings.json").is_file());
    let _ = Command::new("reg")
        .args(["delete", r"HKCU\Software\mouv-tests", "/f"])
        .creation_flags(NO_WINDOW)
        .output();
}

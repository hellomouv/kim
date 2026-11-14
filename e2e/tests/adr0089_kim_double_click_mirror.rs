//! E2E — ADR 0089: Kim from a build of this repository. `KIM-FR-012`, `KIM-SEC-001`.
//!
//! The release `kim.exe` and `subak-gui.exe` carry their icons; `kim` without arguments from a
//! script (pipes, no console window) stays the headless CLI; a local mirror (a test-signed
//! `file://` catalog whose Subak package is a file next to it, as `cargo xtask local-mirror`
//! writes it) installs the release Subak with the debug `kim` (the build that trusts a test key)
//! and leaves the window program Kim Manager's Open button starts. (A `file:` package from a
//! non-file source is refused: `crates/kim/src/catalog/store.rs` tests.)
#![cfg(windows)]

use kim_e2e::{TempDir, bin, bin_debug};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

const SEED: [u8; 32] = [89u8; 32];
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn adr0089_kim_from_a_build_icons_headless_start_and_local_mirror() {
    // Icons and names in the release programs.
    for (exe, ico, name) in [
        (bin("kim"), "assets/icon/kim.ico", "Kim"),
        (
            bin("subak").with_file_name("subak-gui.exe"),
            "assets/subak/subak.ico",
            "Subak",
        ),
    ] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let images = kim_winres::parse_ico(&std::fs::read(root.join(ico)).unwrap()).unwrap();
        let res = kim_winres::resources(&std::fs::read(&exe).unwrap()).unwrap();
        let icons = res.iter().filter(|r| r.0 == kim_winres::RT_ICON).count();
        assert_eq!(icons, images.len(), "{}", exe.display());
        let v = &res
            .iter()
            .find(|r| r.0 == kim_winres::RT_VERSION)
            .unwrap()
            .2;
        assert_eq!(
            kim_winres::version_string(v, "FileDescription").as_deref(),
            Some(name)
        );
    }

    // A script's `kim` without arguments: help, exit 0, no window program started.
    let out = Command::new(bin("kim"))
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage"));

    // The local mirror: the release Subak package next to a test-signed catalog.
    let sb = TempDir::new("adr0089");
    let mirror = sb.path().join("kim-local");
    std::fs::create_dir_all(&mirror).unwrap();
    let exe = bin("subak");
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for (n, p) in [
        ("subak.exe", exe.clone()),
        ("subak-gui.exe", exe.with_file_name("subak-gui.exe")),
    ] {
        z.start_file(n, o).unwrap();
        z.write_all(&std::fs::read(p).unwrap()).unwrap();
    }
    let body = z.finish().unwrap().into_inner();
    let zip_path = mirror.join("subak-0.0.1-windows-x86_64.zip");
    std::fs::write(&zip_path, &body).unwrap();
    let bundled: Value = serde_json::from_slice(kim::catalog::store::BUNDLED_CATALOG).unwrap();
    let mut doc = bundled.clone();
    doc["catalog_version"] = json!(bundled["catalog_version"].as_u64().unwrap() + 1);
    for t in doc["tools"].as_array_mut().unwrap() {
        if t["id"] == "subak" {
            t["releases"] = json!([{"version": "0.0.1", "channel": "stable",
                "published_at": "2026-11-14T00:00:00Z", "release_notes": "built on this PC",
                "assets": [{"os": "windows", "arch": "x86_64", "kind": "zip", "entrypoint": "subak.exe",
                    "url": kim::start::file_url(&zip_path), "size": body.len(),
                    "sha256": kim::catalog::signature::hex(&Sha256::digest(&body))}]}]);
        }
    }
    let bytes = serde_json::to_vec_pretty(&doc).unwrap();
    let catalog = mirror.join("catalog.json");
    std::fs::write(&catalog, &bytes).unwrap();
    std::fs::write(
        mirror.join("catalog.json.sig"),
        serde_json::to_vec(&kim::catalog::signature::sign(&bytes, &SEED)).unwrap(),
    )
    .unwrap();
    let kim = |args: &[&str]| -> Value {
        let out = Command::new(bin_debug("kim"))
            .args(args)
            .arg("--json")
            .env("LOCALAPPDATA", sb.path().join("Local"))
            .env("APPDATA", sb.path().join("Roaming"))
            .env("KIM_NO_INPUT", "1")
            .env(
                "KIM_DEV_TRUSTED_KEY",
                kim::catalog::signature::public_key_hex(&SEED),
            )
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let url = kim::start::file_url(&catalog);
    let r = kim(&["catalog", "refresh", "--source", &url]);
    assert_eq!(r["ok"], true, "{r}");
    let r = kim(&["install", "subak"]);
    assert_eq!(r["ok"], true, "{r}");
    let gui = sb
        .path()
        .join("Local")
        .join("Programs")
        .join("mouv")
        .join("subak")
        .join("versions")
        .join("0.0.1")
        .join("subak-gui.exe");
    assert!(gui.is_file(), "Open Subak starts {}", gui.display());
    println!(
        "EVIDENCE adr0089: Subak installed from the local mirror; {} is there",
        gui.display()
    );
}

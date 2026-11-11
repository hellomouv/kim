//! E2E — Step 40: the Windows release lifecycle with the real Subak package (docs/RELEASING.md,
//! docs/INSTALL.md). `KIM-SEC-001`, `KIM-SEC-003`, `SUBAK-SEC-001`.
//!
//! The release `subak.exe` and `subak-gui.exe`, zipped as `cargo xtask package subak` lays them
//! out (programs, capabilities.json, licenses), offered by a test-signed catalog over loopback to
//! the debug `kim` (the only build that trusts a test key). A user installs Subak explicitly,
//! starts a Terminal from the installed program, is refused an uninstall while the Host runs,
//! updates side by side without stopping it (the running Terminal survives; the new CLI talks to
//! the old Host), rolls back and forward again, is refused a refresh in offline mode, and
//! uninstalls keeping the user data.
#![cfg(windows)]

use kim_e2e::{TempDir, bin, bin_debug};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

const SEED: [u8; 32] = [44u8; 32];

fn serve(files: Vec<(&'static str, Vec<u8>)>) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            if let Some((_, body)) = files.iter().find(|(p, _)| *p == path) {
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(body);
            } else {
                let _ = s.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        }
    });
    port
}

/// The release layout of Subak at `version` (the programs are the release build).
fn subak_package(version: &str) -> Vec<u8> {
    let exe = bin("subak");
    let gui = exe.with_file_name("subak-gui.exe");
    let caps = Command::new(&exe)
        .args(["capabilities", "--json"])
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    let mut doc: Value = serde_json::from_slice(&caps.stdout).unwrap();
    doc["data"]["version"] = json!(version);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for (name, bytes) in [
        ("subak.exe", std::fs::read(&exe).unwrap()),
        ("subak-gui.exe", std::fs::read(&gui).unwrap()),
        (
            "capabilities.json",
            serde_json::to_vec_pretty(&doc["data"]).unwrap(),
        ),
        (
            "LICENSE-MIT",
            std::fs::read(root.join("LICENSE-MIT")).unwrap(),
        ),
        (
            "LICENSE-APACHE",
            std::fs::read(root.join("LICENSE-APACHE")).unwrap(),
        ),
    ] {
        z.start_file(name, o).unwrap();
        z.write_all(&bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn release(v: &str, url: &str, body: &[u8]) -> Value {
    json!({"version": v, "channel": "stable", "published_at": "2026-11-12T00:00:00Z", "release_notes": v,
        "assets": [{"os": "windows", "arch": "x86_64", "url": url, "size": body.len(),
        "sha256": kim::catalog::signature::hex(&Sha256::digest(body)), "kind": "zip", "entrypoint": "subak.exe"}]})
}

fn publish(sandbox: &Path, version: u64, releases: &[Value]) {
    let doc = json!({"schema": "kim.catalog", "schema_version": 1, "catalog_version": version,
        "generated_at": "2026-11-12T00:00:00Z", "expires_at": "2099-01-01T00:00:00Z", "publisher": "e2e",
        "tools": [{"id": "subak", "name": "Subak", "summary": "s", "description": "d", "license": "MIT OR Apache-2.0",
                   "support": "stable", "releases": releases}]});
    let bytes = serde_json::to_vec_pretty(&doc).unwrap();
    let sig = serde_json::to_vec(&kim::catalog::signature::sign(&bytes, &SEED)).unwrap();
    let dir = sandbox
        .join("Local")
        .join("mouv")
        .join("kim")
        .join("catalog");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("catalog.json"), bytes).unwrap();
    std::fs::write(dir.join("catalog.json.sig"), sig).unwrap();
}

fn run(exe: &Path, sandbox: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(exe)
        .args(args)
        .env("LOCALAPPDATA", sandbox.join("Local"))
        .env("APPDATA", sandbox.join("Roaming"))
        .env(
            "KIM_DEV_TRUSTED_KEY",
            kim::catalog::signature::public_key_hex(&SEED),
        )
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn subak_installs_updates_rolls_back_and_uninstalls_without_touching_running_work() {
    let sb = TempDir::new("release40");
    let p = sb.path();
    std::fs::create_dir_all(p.join("Roaming")).unwrap();
    let kim = bin_debug("kim");
    let (v1, v2) = (subak_package("0.0.1"), subak_package("0.0.2"));
    let port = serve(vec![("/s1.zip", v1.clone()), ("/s2.zip", v2.clone())]);
    let url = |f: &str| format!("http://127.0.0.1:{port}/{f}");

    // First launch: Kim installs nothing by itself.
    let (code, v) = run(&kim, p, &["tools", "list", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        v["data"]["tools"].as_array().unwrap().len(),
        1,
        "only Kim itself: {v}"
    );

    // Install explicitly.
    publish(p, 100, &[release("0.0.1", &url("s1.zip"), &v1)]);
    let (code, v) = run(&kim, p, &["install", "subak", "--json"]);
    assert_eq!(code, 0, "{v}");
    let installed = v["data"]["result"]["entrypoint"]
        .as_str()
        .unwrap()
        .to_string();
    let subak = Path::new(&installed).to_path_buf();
    assert!(subak.is_file());
    let (_, v) = run(&kim, p, &["tools", "describe", "subak", "--json"]);
    assert_eq!(v["data"]["capabilities_source"], "manifest", "{v}");

    // Work in the installed Subak.
    let ep = format!("subak-e2e40-{}", std::process::id());
    let (code, v) = run(&subak, p, &["--json", "--endpoint", &ep, "new", "work"]);
    assert_eq!(code, 0, "{v}");

    // While the Host runs: an uninstall is refused; an update installs side by side and stops
    // nothing — the running Host keeps its files and its Terminal, and the new CLI talks to it.
    publish(
        p,
        101,
        &[
            release("0.0.1", &url("s1.zip"), &v1),
            release("0.0.2", &url("s2.zip"), &v2),
        ],
    );
    let (code, v) = run(&kim, p, &["uninstall", "subak", "--yes", "--json"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("tool_running")),
        "{v}"
    );
    let (code, v) = run(&kim, p, &["update", "subak", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["to"], "0.0.2");
    assert!(
        !v["data"]["result"]["running"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{v}"
    );
    assert!(subak.is_file(), "the running version's files are kept");
    let new = std::path::PathBuf::from(v["data"]["result"]["entrypoint"].as_str().unwrap());
    let (_, v) = run(&new, p, &["--json", "--endpoint", &ep, "info", "work"]);
    assert_eq!(
        v["data"]["terminal"]["state"], "running",
        "the new CLI and the old Host negotiate: {v}"
    );

    // Roll back: a dry run first, then for real (also while the Host runs), and back again.
    let (code, v) = run(&kim, p, &["rollback", "subak", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        (
            v["data"]["plan"]["from"].as_str(),
            v["data"]["plan"]["to"].as_str()
        ),
        (Some("0.0.2"), Some("0.0.1"))
    );
    let (code, v) = run(&kim, p, &["rollback", "subak", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["to"], "0.0.1");
    let (_, v) = run(&subak, p, &["--json", "--endpoint", &ep, "info", "work"]);
    assert_eq!(
        v["data"]["terminal"]["state"], "running",
        "nothing was stopped: {v}"
    );
    let (code, v) = run(&kim, p, &["rollback", "subak", "--json"]);
    assert_eq!(
        (code, v["data"]["result"]["to"].as_str()),
        (0, Some("0.0.2")),
        "a rollback can be undone: {v}"
    );
    run(
        &subak,
        p,
        &[
            "--json",
            "--endpoint",
            &ep,
            "host",
            "stop",
            "--kill-terminals",
            "--yes",
        ],
    );
    std::thread::sleep(std::time::Duration::from_millis(800));
    let (_, list) = run(&kim, p, &["tools", "list", "--json"]);
    let installed_now = std::path::PathBuf::from(
        list["data"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "subak")
            .unwrap()["entrypoint"]
            .as_str()
            .unwrap(),
    );
    assert!(installed_now.display().to_string().contains("0.0.2"));

    // Offline: no network operation.
    run(
        &kim,
        p,
        &["settings", "set", "offline_mode", "true", "--json"],
    );
    let (code, v) = run(
        &kim,
        p,
        &[
            "catalog",
            "refresh",
            "--source",
            &url("catalog.json"),
            "--json",
        ],
    );
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (6, Some("offline_mode")),
        "{v}"
    );

    // Uninstall keeps the user data.
    let data = p.join("Roaming").join("mouv").join("subak");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("config.toml"), "# mine\n").unwrap();
    let (code, v) = run(&kim, p, &["uninstall", "subak", "--yes", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert!(!subak.exists() && !installed_now.exists());
    assert!(data.join("config.toml").is_file(), "user data kept");
}

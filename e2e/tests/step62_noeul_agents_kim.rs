//! E2E — Step 62: Noeul's headless contract, agents and Kim (docs/NOEUL_CLI.md, docs/adr/0076-noeul-agents-and-kim.md).
//! Requirements: NOEUL-FR-110, NOEUL-FR-111, NOEUL-FR-112, NOEUL-FR-113, NOEUL-SEC-010,
//! KIM-FR-020, KIM-SEC-001.
//!
//! The release `noeul.exe`, packaged as `cargo xtask package noeul` lays it out, offered by a
//! test-signed catalog over loopback to the debug `kim`: a user installs Noeul explicitly; Kim
//! finds it as the provider of `image.pin` and `screen.capture-file`; `kim tools invoke pin.list`
//! reaches it and is told Noeul is not running (an interop call never starts it); the installed
//! Noeul works on its own (`capabilities`, `mcp tools`, `config show`) and its privacy defaults
//! are notify for the CLI and off for agents; `capture --output` needs a running Noeul and a
//! local absolute `.png`; Kim uninstalls it. Nothing is captured from the screen; no window opens.
#![cfg(windows)]

use kim_e2e::{TempDir, bin, bin_debug};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

const SEED: [u8; 32] = [62u8; 32];

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

/// The release layout of Noeul (`cargo xtask package noeul`): the release `noeul.exe`, its
/// capabilities document and the licenses.
fn noeul_package(version: &str) -> Vec<u8> {
    let exe = bin("noeul");
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
        ("noeul.exe", std::fs::read(&exe).unwrap()),
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
        "sha256": kim::catalog::signature::hex(&Sha256::digest(body)), "kind": "zip", "entrypoint": "noeul.exe"}]})
}

fn publish(sandbox: &Path, version: u64, releases: &[Value]) {
    let doc = json!({"schema": "kim.catalog", "schema_version": 1, "catalog_version": version,
        "generated_at": "2026-11-12T00:00:00Z", "expires_at": "2099-01-01T00:00:00Z", "publisher": "e2e",
        "tools": [{"id": "noeul", "name": "Noeul", "summary": "s", "description": "d", "license": "MIT OR Apache-2.0",
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
fn kim_installs_noeul_which_provides_interop_and_runs_on_its_own() {
    let sb = TempDir::new("noeul62");
    let p = sb.path();
    std::fs::create_dir_all(p.join("Roaming")).unwrap();
    let kim = bin_debug("kim");
    let pkg = noeul_package("0.0.1");
    let port = serve(vec![("/n1.zip", pkg.clone())]);
    publish(
        p,
        100,
        &[release(
            "0.0.1",
            &format!("http://127.0.0.1:{port}/n1.zip"),
            &pkg,
        )],
    );
    let (code, v) = run(&kim, p, &["install", "noeul", "--json"]);
    assert_eq!(code, 0, "{v}");
    let noeul = Path::new(v["data"]["result"]["entrypoint"].as_str().unwrap()).to_path_buf();
    assert!(noeul.is_file());
    let (_, v) = run(&kim, p, &["tools", "describe", "noeul", "--json"]);
    assert_eq!(v["data"]["capabilities_source"], "manifest", "{v}");

    // Kim finds Noeul as a provider; an interop call never starts it.
    for cap in ["pin.list", "image.pin", "screen.capture-file"] {
        let (code, v) = run(&kim, p, &["tools", "find", cap, "--json"]);
        assert_eq!(
            (code, v["data"]["providers"][0]["tool"].as_str()),
            (0, Some("noeul")),
            "{cap}: {v}"
        );
    }
    let (code, v) = run(&kim, p, &["tools", "invoke", "pin.list", "--json"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (1, Some("tool_failed")),
        "{v}"
    );
    assert!(v.to_string().contains("noeul_not_running"), "{v}");
    let (code, v) = run(
        &kim,
        p,
        &[
            "tools",
            "invoke",
            "image.pin",
            "--params",
            r#"{"path":"C:/x.png"}"#,
            "--json",
        ],
    );
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("confirmation_required")),
        "write needs consent: {v}"
    );

    // The installed Noeul on its own: discoverable, agents off, nothing started.
    let (code, v) = run(&noeul, p, &["capabilities", "--json"]);
    assert_eq!(code, 0);
    assert!(
        v["data"]["provides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "screen.capture-file")
    );
    let (_, v) = run(&noeul, p, &["mcp", "tools", "--json"]);
    let tools: Vec<&str> = v["data"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        ["noeul_status", "noeul_pin_list", "noeul_history_list"],
        "read only without grants"
    );
    let (_, v) = run(&noeul, p, &["config", "show", "--json"]);
    assert_eq!(
        v["data"]["settings"]["privacy"],
        json!({"cli": "notify", "mcp_captures": "off"})
    );
    let (code, v) = run(
        &noeul,
        p,
        &[
            "capture",
            "--region",
            "0,0,4,4",
            "--output",
            r"C:\x.png",
            "--json",
        ],
    );
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (3, Some("noeul_not_running")),
        "{v}"
    );
    let (_, v) = run(&noeul, p, &["status", "--json"]);
    assert_eq!(v["data"]["running"], false, "nothing started Noeul");

    let (code, v) = run(&kim, p, &["uninstall", "noeul", "--yes", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert!(!noeul.exists());
}

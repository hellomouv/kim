//! E2E — Step 81: Ddalgak for agents and Kim (docs/DDALGAK_MCP.md, ADR 0082). Requirements:
//! `DDALGAK-FR-060`, `DDALGAK-FR-062`, `DDALGAK-SEC-006`, `KIM-FR-020`, `KIM-SEC-001`.
//!
//! The release `ddalgak.exe`, packaged as `cargo xtask package ddalgak` lays it out, offered by a
//! test-signed catalog over loopback to the debug `kim`: a user installs Ddalgak explicitly; Kim
//! finds it as the provider of `keys.explain`, `rules.list`, `mode.status` and `mode.set`;
//! `kim tools invoke keys.explain` answers from the rules alone (no app, nothing pressed);
//! `mode.set` needs the user's consent (`confirmation_required`). The installed Ddalgak's MCP
//! adapter, driven over stdin like an MCP client: initialize, only reading tools without a grant,
//! explain works, an ungranted change is refused, hostile arguments never reach the command line,
//! a granted dry-run pause is passed on, and every call is audited without content. Kim
//! uninstalls it. No app is started and no window opens.
#![cfg(windows)]

use kim_e2e::{TempDir, bin, bin_debug, require_e2e};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const SEED: [u8; 32] = [81u8; 32];

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

/// The package layout of `cargo xtask package ddalgak`, with the release CLI (the tray app and
/// the editor are not needed by this scenario).
fn package(version: &str) -> Vec<u8> {
    let exe = bin("ddalgak");
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
        ("ddalgak.exe", std::fs::read(&exe).unwrap()),
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

fn publish(sandbox: &Path, url: &str, body: &[u8]) {
    let doc = json!({"schema": "kim.catalog", "schema_version": 1, "catalog_version": 100,
        "generated_at": "2026-11-12T00:00:00Z", "expires_at": "2099-01-01T00:00:00Z", "publisher": "e2e",
        "tools": [{"id": "ddalgak", "name": "Ddalgak", "summary": "s", "description": "d", "license": "MIT OR Apache-2.0",
            "support": "stable", "releases": [{"version": "0.0.1", "channel": "stable", "published_at": "2026-11-12T00:00:00Z",
                "release_notes": "e2e", "assets": [{"os": "windows", "arch": "x86_64", "url": url, "size": body.len(),
                "sha256": kim::catalog::signature::hex(&Sha256::digest(body)), "kind": "zip", "entrypoint": "ddalgak.exe"}]}]}]});
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

fn cmd(exe: &Path, sandbox: &Path) -> Command {
    let mut c = Command::new(exe);
    c.env("LOCALAPPDATA", sandbox.join("Local"))
        .env("APPDATA", sandbox.join("Roaming"))
        .env(
            "KIM_DEV_TRUSTED_KEY",
            kim::catalog::signature::public_key_hex(&SEED),
        )
        .creation_flags(0x0800_0000);
    c
}

fn run(exe: &Path, sandbox: &Path, args: &[&str]) -> (i32, Value) {
    let out = cmd(exe, sandbox).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

/// One MCP session over stdin/stdout; the replies in order.
fn mcp(exe: &Path, sandbox: &Path, allow: &[&str], messages: &[Value]) -> Vec<Value> {
    let mut c = cmd(exe, sandbox);
    c.args(["mcp", "serve"]);
    for a in allow {
        c.args(["--allow", a]);
    }
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for m in messages {
            writeln!(stdin, "{m}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn call(id: u64, name: &str, args: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}})
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step81_kim_installs_ddalgak_which_serves_interop_and_a_guarded_mcp_adapter() {
    require_e2e();
    let sb = TempDir::new("ddalgak81");
    let p = sb.path();
    std::fs::create_dir_all(p.join("Roaming")).unwrap();
    let kim = bin_debug("kim");
    let pkg = package("0.0.1");
    let port = serve(vec![("/d1.zip", pkg.clone())]);
    publish(p, &format!("http://127.0.0.1:{port}/d1.zip"), &pkg);
    let (code, v) = run(&kim, p, &["install", "ddalgak", "--json"]);
    assert_eq!(code, 0, "{v}");
    let ddalgak = Path::new(v["data"]["result"]["entrypoint"].as_str().unwrap()).to_path_buf();
    assert!(ddalgak.is_file());

    // Kim finds Ddalgak's capabilities and invokes them through `ddalgak interop invoke`.
    for cap in ["keys.explain", "rules.list", "mode.status", "mode.set"] {
        let (code, v) = run(&kim, p, &["tools", "find", cap, "--json"]);
        assert_eq!(
            (code, v["data"]["providers"][0]["tool"].as_str()),
            (0, Some("ddalgak")),
            "{cap}: {v}"
        );
    }
    let (code, v) = run(
        &kim,
        p,
        &[
            "tools",
            "invoke",
            "keys.explain",
            "--params",
            r#"{"keys":"Ctrl+Alt+Shift+F21"}"#,
            "--json",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        v["data"]["result"]["decision"]["outcome"], "no_match",
        "{v}"
    );
    let (code, v) = run(&kim, p, &["tools", "invoke", "rules.list", "--json"]);
    assert_eq!(code, 0, "{v}");
    let (code, v) = run(
        &kim,
        p,
        &[
            "tools",
            "invoke",
            "mode.set",
            "--params",
            r#"{"mode":"base"}"#,
            "--json",
        ],
    );
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("confirmation_required")),
        "a write needs consent: {v}"
    );

    // The MCP adapter of the installed Ddalgak.
    let r = mcp(
        &ddalgak,
        p,
        &[],
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            call(3, "ddalgak_explain", &json!({"keys": "F13"})),
            call(4, "ddalgak_pause", &json!({})),
            call(5, "ddalgak_rule_show", &json!({"id": "--show-text"})),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ],
    );
    assert_eq!(r.len(), 5, "a notification gets no reply: {r:?}");
    assert_eq!(r[0]["result"]["protocolVersion"], "2025-06-18");
    let names: Vec<&str> = r[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"ddalgak_explain")
            && !names
                .iter()
                .any(|n| n.contains("pause") || n.contains("enable")),
        "{names:?}"
    );
    assert_eq!(r[2]["result"]["isError"], false, "{}", r[2]);
    assert_eq!(
        r[2]["result"]["structuredContent"]["decision"]["outcome"],
        "no_match"
    );
    assert_eq!(r[3]["error"]["code"], -32602, "ungranted: {}", r[3]);
    assert_eq!(
        r[4]["result"]["structuredContent"]["error"]["code"], "usage",
        "hostile id: {}",
        r[4]
    );
    // With the grant, a dry-run pause reaches the CLI (no app runs: engine_not_running).
    let r = mcp(
        &ddalgak,
        p,
        &["engine"],
        &[call(1, "ddalgak_pause", &json!({"dry_run": true}))],
    );
    assert_eq!(
        r[0]["result"]["structuredContent"]["error"]["code"], "engine_not_running",
        "{}",
        r[0]
    );
    let audit = std::fs::read_to_string(p.join("Local/mouv/ddalgak/mcp-audit.jsonl")).unwrap();
    let outcomes: Vec<String> = audit
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            format!(
                "{}:{}",
                v["tool"].as_str().unwrap(),
                v["outcome"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        outcomes,
        [
            "ddalgak_explain:ok",
            "ddalgak_pause:not_offered",
            "ddalgak_rule_show:usage",
            "ddalgak_pause:engine_not_running"
        ]
    );

    let (code, v) = run(&kim, p, &["uninstall", "ddalgak", "--yes", "--json"]);
    assert_eq!(code, 0, "{v}");
    assert!(!ddalgak.exists());
}

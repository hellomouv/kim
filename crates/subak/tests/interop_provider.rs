//! Step 38 — Subak as a Kim interop provider (ADR 0071). `KIM-FR-020`, `KIM-SEC-001`.
//!
//! The real `subak interop invoke --json --no-input` with requests on stdin, as Kim's
//! `tools invoke` sends them: listing without a Host (none started), a dry-run open, a real
//! open, a file path typed into the pane, refused programs and paths, unknown capabilities and
//! interface versions.
#![cfg(windows)]
#![allow(unsafe_code)]
#![allow(clippy::needless_pass_by_value)] // test helpers take owned JSON for brevity

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::{Value, json};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

fn invoke(
    ep: &str,
    appdata: &std::path::Path,
    cap: &str,
    iface: &str,
    params: Value,
    dry: bool,
) -> (i32, Value) {
    let mut c = Command::new(subak())
        .args(["interop", "invoke", "--json", "--no-input"])
        .env("SUBAK_ENDPOINT", ep)
        .env("APPDATA", appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .unwrap();
    let req = json!({"schema": "kim.interop.request", "schema_version": 1, "capability": cap, "interface": iface, "params": params, "dry_run": dry, "caller": {"tool": "test", "version": "1.0.0"}});
    c.stdin
        .take()
        .unwrap()
        .write_all(req.to_string().as_bytes())
        .unwrap();
    let out = c.wait_with_output().unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    validate("subak-cli-v1", &doc);
    (out.status.code().unwrap_or(-1), doc)
}

#[test]
fn subak_serves_terminal_capabilities_to_other_tools() {
    let id = std::process::id();
    let ep = endpoint(&format!("w38-{id}"));
    let appdata = std::env::temp_dir().join(format!("subak-t38-{id}"));
    std::fs::create_dir_all(&appdata).unwrap();
    let stop = || {
        let _ = run(&[
            "host",
            "stop",
            "--endpoint",
            &ep,
            "--kill-terminals",
            "--yes",
        ]);
    };

    // Reading starts nothing.
    let (code, d) = invoke(&ep, &appdata, "terminal.list", "1", json!({}), false);
    assert_eq!(code, 0, "{d}");
    assert_eq!(d["data"]["host_running"], false);
    let (code, _) = run(&["host", "status", "--endpoint", &ep]);
    assert_eq!(code, 3);

    // A dry run checks and creates nothing; a real open creates the Terminal.
    let (code, d) = invoke(
        &ep,
        &appdata,
        "terminal.open",
        "1",
        json!({"name": "from-noeul"}),
        true,
    );
    assert_eq!(code, 0, "{d}");
    assert_eq!(d["data"]["dry_run"], true);
    let (code, d) = invoke(
        &ep,
        &appdata,
        "terminal.open",
        "1",
        json!({"name": "from-noeul"}),
        false,
    );
    assert_eq!(code, 0, "{d}");
    assert_eq!(d["data"]["name"], "from-noeul");
    let (_, d) = invoke(&ep, &appdata, "terminal.list", "1", json!({}), false);
    assert_eq!(d["data"]["terminals"][0]["name"], "from-noeul", "{d}");

    // A screenshot's path typed into the pane (no Enter): the shell shows it.
    let shot = appdata.join("shot 1.png");
    std::fs::write(&shot, b"png").unwrap();
    let (code, d) = invoke(
        &ep,
        &appdata,
        "terminal.paste-path",
        "1",
        json!({"target": "from-noeul", "path": shot.display().to_string()}),
        false,
    );
    assert_eq!(code, 0, "{d}");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let (_, c) = run(&["capture", "from-noeul", "--endpoint", &ep]);
        let text = c["data"]["lines"].to_string();
        if text.contains("shot 1.png") {
            break;
        }
        assert!(
            std::time::Instant::now() < end,
            "the path never appeared: {text}"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // Refusals.
    let (code, d) = invoke(
        &ep,
        &appdata,
        "terminal.open",
        "1",
        json!({"command": ["calc.exe"]}),
        false,
    );
    assert_eq!(
        (code, d["error"]["code"].as_str()),
        (2, Some("usage")),
        "{d}"
    );
    let (code, d) = invoke(
        &ep,
        &appdata,
        "terminal.paste-path",
        "1",
        json!({"target": "from-noeul", "path": "C:\\no\\such\\file.png"}),
        false,
    );
    assert_ne!(code, 0, "{d}");
    let (code, d) = invoke(&ep, &appdata, "shell.exec", "1", json!({}), false);
    assert_eq!(
        (code, d["error"]["code"].as_str()),
        (5, Some("capability_missing"))
    );
    let (code, d) = invoke(&ep, &appdata, "terminal.open", "2", json!({}), false);
    assert_eq!(
        (code, d["error"]["code"].as_str()),
        (5, Some("tool_incompatible"))
    );
    stop();
    let _ = std::fs::remove_dir_all(&appdata);
}

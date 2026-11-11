//! E2E — Step 38: one Kim tool invoking another (ADR 0071). `KIM-FR-020`, `KIM-SEC-001`,
//! `KIM-NFR-001`.
//!
//! The release `kim` and `subak`, with `subak` laid out in a sandboxed programs folder as Kim's
//! installer would leave it (program, install record, capabilities manifest). An agent finds
//! who provides `terminal.open`, lists Terminals (no Host started), is refused a write without
//! consent, opens a Terminal with `--allow-write`, and types a screenshot's path into it. No
//! resident Kim process exists at any point; nothing is installed.
#![cfg(windows)]

use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const NO_WINDOW: u32 = 0x0800_0000;

fn run(exe: &Path, args: &[&str], env: &[(&str, &Path)], extra: &[(&str, &str)]) -> (i32, Value) {
    let mut c = Command::new(exe);
    c.args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(NO_WINDOW);
    for (k, v) in env {
        c.env(k, v);
    }
    for (k, v) in extra {
        c.env(k, v);
    }
    let out = c.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn kim_finds_and_invokes_subaks_terminal_capabilities() {
    let id = std::process::id();
    let root = std::env::temp_dir().join(format!("kim-e2e38-{id}"));
    let _ = std::fs::remove_dir_all(&root);
    let (local, roaming) = (root.join("Local"), root.join("Roaming"));
    std::fs::create_dir_all(&roaming).unwrap();
    let env = [
        ("LOCALAPPDATA", local.as_path()),
        ("APPDATA", roaming.as_path()),
    ];
    let ep = format!("subak-e2e38-{id}");
    let extra = [("SUBAK_ENDPOINT", ep.as_str())];
    let kim = kim_e2e::bin("kim");
    let subak_src = kim_e2e::bin("subak");

    // `subak` as an installed tool: program, record, manifest.
    let (_, caps) = run(&subak_src, &["capabilities", "--json"], &[], &[]);
    let version = caps["data"]["version"].as_str().unwrap().to_string();
    let tdir = local.join("Programs").join("mouv").join("subak");
    let vdir = tdir.join("versions").join(&version);
    std::fs::create_dir_all(&vdir).unwrap();
    let subak = vdir.join("subak.exe");
    std::fs::copy(&subak_src, &subak).unwrap();
    std::fs::write(
        vdir.join("capabilities.json"),
        serde_json::to_vec_pretty(&caps["data"]).unwrap(),
    )
    .unwrap();
    std::fs::write(
        tdir.join("current.json"),
        json!({"schema": "kim.install", "schema_version": 1, "tool": "subak", "version": version,
               "entrypoint": format!("versions/{version}/subak.exe"), "installed_at": "2026-11-11T00:00:00Z",
               "sha256": "0".repeat(64), "source_url": "file:///e2e", "catalog_version": 1, "previous_version": null})
        .to_string(),
    )
    .unwrap();

    let k = |args: &[&str]| {
        let mut a = vec!["--json"];
        a.extend_from_slice(args);
        run(&kim, &a, &env, &extra)
    };
    let (code, v) = k(&["tools", "find", "terminal.open"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["providers"][0]["tool"], "subak", "{v}");
    let (code, v) = k(&["tools", "invoke", "terminal.list"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["host_running"], false);
    let open = [
        "tools",
        "invoke",
        "terminal.open",
        "--params",
        r#"{"name":"from-kim"}"#,
    ];
    let (code, v) = k(&open);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (7, Some("confirmation_required")),
        "{v}"
    );
    let mut go = open.to_vec();
    go.push("--allow-write");
    let (code, v) = k(&go);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["name"], "from-kim");
    let shot = root.join("noeul-capture.png");
    std::fs::write(&shot, b"png").unwrap();
    let params = json!({"target": "from-kim", "path": shot.display().to_string()}).to_string();
    let (code, v) = k(&[
        "tools",
        "invoke",
        "terminal.paste-path",
        "--params",
        &params,
        "--allow-write",
    ]);
    assert_eq!(code, 0, "{v}");
    let (code, v) = k(&["tools", "invoke", "capture.region"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (5, Some("capability_missing"))
    );

    let _ = run(
        &subak,
        &[
            "--json",
            "--endpoint",
            &ep,
            "host",
            "stop",
            "--kill-terminals",
            "--yes",
        ],
        &env,
        &[],
    );
    let _ = std::fs::remove_dir_all(&root);
}

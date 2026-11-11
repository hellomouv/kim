//! E2E — Step 37: the MCP server (ADR 0070). `SUBAK-FR-062`, `KIM-FR-021`, `KIM-SEC-013`.
//!
//! An agent starts the release `subak mcp serve --allow create --allow input --allow
//! destructive` over stdio: it lists tools, creates a shell, types a command, waits for its
//! output, reads the screen and kills the Terminal only after confirming.
#![cfg(windows)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn an_agent_drives_subak_over_mcp() {
    let id = std::process::id();
    let ep = format!("subak-e2e37-{id}");
    let appdata = std::env::temp_dir().join(format!("subak-e2e37-{id}"));
    std::fs::create_dir_all(&appdata).unwrap();
    let exe = kim_e2e::bin("subak");
    let mut child = Command::new(&exe)
        .args(["mcp", "serve", "--endpoint", &ep])
        .args([
            "--allow",
            "create",
            "--allow",
            "input",
            "--allow",
            "destructive",
        ])
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .unwrap();
    let mut tx = child.stdin.take().unwrap();
    let mut rx = BufReader::new(child.stdout.take().unwrap());
    let mut n = 0u64;
    let mut call = |method: &str, params: Value| -> Value {
        n += 1;
        writeln!(
            tx,
            "{}",
            json!({"jsonrpc": "2.0", "id": n, "method": method, "params": params})
        )
        .unwrap();
        tx.flush().unwrap();
        loop {
            let mut l = String::new();
            assert!(rx.read_line(&mut l).unwrap() > 0, "the server ended");
            let v: Value = serde_json::from_str(&l).unwrap();
            if v["id"] == n {
                return v;
            }
        }
    };
    let init = call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "1"}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "subak");
    let tools = call("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"send_text") && !names.contains(&"recover_terminal"),
        "{names:?}"
    );
    let tool = |call: &mut dyn FnMut(&str, Value) -> Value, name: &str, args: Value| -> Value {
        let r = call("tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], false, "{name}: {r}");
        r["result"]["structuredContent"].clone()
    };
    tool(&mut call, "create_terminal", json!({"name": "agent"}));
    tool(
        &mut call,
        "send_text",
        json!({"target": "agent", "text": "echo e2e-mcp-%OS%"}),
    );
    let w = tool(
        &mut call,
        "wait_for_text",
        json!({"target": "agent", "text": "e2e-mcp-Windows_NT", "timeout_s": 30}),
    );
    assert_eq!(w["found"], true);
    let s = tool(&mut call, "read_screen", json!({"target": "agent"}));
    assert!(s["text"].as_str().unwrap().contains("e2e-mcp-Windows_NT"));
    let preview = tool(&mut call, "kill_terminal", json!({"target": "agent"}));
    assert_eq!(preview["dry_run"], true);
    tool(
        &mut call,
        "kill_terminal",
        json!({"target": "agent", "confirm": true}),
    );
    drop(tx);
    let _ = child.wait();
    let _ = Command::new(&exe)
        .args([
            "--json",
            "--endpoint",
            &ep,
            "host",
            "stop",
            "--kill-terminals",
            "--yes",
        ])
        .stdout(Stdio::null())
        .creation_flags(0x0800_0000)
        .status();
    let _ = std::fs::remove_dir_all(&appdata);
}

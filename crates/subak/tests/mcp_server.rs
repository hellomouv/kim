//! Step 37 — the MCP server (ADR 0070). `SUBAK-FR-062`, `KIM-FR-021`, `KIM-SEC-013`, `KIM-FR-012`.
//!
//! The real `subak mcp serve` over stdio against a real Host: the handshake, read-only tools
//! (a screen with a secret comes back redacted), ungranted tools absent and refused, granted
//! tools creating a Terminal and typing into it, destructive tools previewing until confirmed,
//! a cancelled wait, malformed messages, and an audit trail without the typed text.
#![cfg(windows)]
#![allow(unsafe_code)]
#![allow(clippy::needless_pass_by_value)] // test helpers take owned JSON for brevity

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

struct Mcp {
    child: Child,
    tx: ChildStdin,
    rx: Receiver<Value>,
    next: u64,
}

impl Mcp {
    fn start(ep: &str, appdata: &std::path::Path, grants: &[&str]) -> Self {
        let mut c = Command::new(subak());
        c.args(["mcp", "serve", "--endpoint", ep]);
        for g in grants {
            c.args(["--allow", g]);
        }
        let mut child = c
            .env("APPDATA", appdata)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let tx = child.stdin.take().unwrap();
        let out = child.stdout.take().unwrap();
        let (stx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(l) = line else { break };
                let v: Value = serde_json::from_str(&l).expect("every line is one JSON message");
                if stx.send(v).is_err() {
                    break;
                }
            }
        });
        let mut m = Self {
            child,
            tx,
            rx,
            next: 1,
        };
        let r = m.req(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "subak-test", "version": "1"}}),
        );
        assert_eq!(r["result"]["serverInfo"]["name"], "subak", "{r}");
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        m.notify("notifications/initialized", json!({}));
        m
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.tx, "{v}").unwrap();
        self.tx.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn send_req(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    fn wait(&mut self, id: u64) -> Value {
        let end = Instant::now() + Duration::from_secs(70);
        loop {
            let v = self
                .rx
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .expect("an answer");
            if v["id"] == id {
                return v;
            }
        }
    }

    fn req(&mut self, method: &str, params: Value) -> Value {
        let id = self.send_req(method, params);
        self.wait(id)
    }

    /// `tools/call`: (isError, structuredContent).
    fn call(&mut self, name: &str, args: Value) -> (bool, Value) {
        let r = self.req("tools/call", json!({"name": name, "arguments": args}));
        let res = &r["result"];
        assert!(res["content"][0]["text"].is_string(), "{r}");
        (res["isError"] == true, res["structuredContent"].clone())
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        let (err, v) = self.call(name, args.clone());
        assert!(!err, "{name} {args}: {v}");
        v
    }

    fn tool_names(&mut self) -> Vec<String> {
        let r = self.req("tools/list", json!({}));
        r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Host(String);

impl Drop for Host {
    fn drop(&mut self) {
        let _ = run(&[
            "host",
            "stop",
            "--endpoint",
            &self.0,
            "--kill-terminals",
            "--yes",
        ]);
    }
}

#[test]
fn mcp_reads_by_default_and_mutates_only_what_was_granted() {
    let id = std::process::id();
    let ep = endpoint(&format!("w37-{id}"));
    let host = Host(ep.clone());
    let appdata = std::env::temp_dir().join(format!("subak-mcp37-{id}"));
    let _ = std::fs::remove_dir_all(&appdata);
    std::fs::create_dir_all(&appdata).unwrap();

    // Read-only: no Host yet, nothing is started by reading.
    let mut ro = Mcp::start(&ep, &appdata, &[]);
    let names = ro.tool_names();
    assert!(names.contains(&"read_screen".to_string()), "{names:?}");
    for n in [
        "create_terminal",
        "send_text",
        "kill_terminal",
        "recover_terminal",
    ] {
        assert!(
            !names.contains(&n.to_string()),
            "{n} offered without a grant"
        );
    }
    let v = ro.ok("list_terminals", json!({}));
    assert_eq!(v["host_running"], false);
    let (code, _) = run(&["host", "status", "--endpoint", &ep]);
    assert_eq!(code, 3, "reading started no Host");

    // A Terminal made by the user, with a secret on its screen.
    let (code, doc) = run(&["new", "work", "--endpoint", &ep]);
    assert_eq!(code, 0, "{doc}");
    let (code, _) = run(&[
        "command",
        "--endpoint",
        &ep,
        "--target",
        "work",
        "send-keys -l 'echo GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789' Enter",
    ]);
    assert_eq!(code, 0);
    let end = Instant::now() + Duration::from_secs(20);
    let screen = loop {
        let v = ro.ok("read_screen", json!({"target": "work"}));
        if v["redacted"].as_u64().unwrap_or(0) >= 2 || Instant::now() > end {
            break v;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let text = screen["text"].as_str().unwrap();
    assert!(
        !text.contains("ghp_abcdefghijklmnop"),
        "the secret is never shown: {text}"
    );
    assert!(text.contains("***"), "{text}");
    let v = ro.ok("list_terminals", json!({}));
    assert_eq!(v["terminals"][0]["name"], "work", "{v}");
    ro.ok("terminal_info", json!({"target": "work"}));
    ro.ok("list_panes", json!({"target": "work"}));
    ro.ok("claude_status", json!({}));
    let s = ro.ok("host_stats", json!({}));
    assert_eq!(s["host_running"], true);
    ro.ok("list_recoverable", json!({}));
    // Refused: a tool not granted (even if the client calls it anyway), unknown tools, junk.
    let (err, v) = ro.call("send_text", json!({"target": "work", "text": "calc"}));
    assert!(err && v["error"]["code"] == "permission_denied", "{v}");
    let r = ro.req("tools/call", json!({"name": "exec", "arguments": {}}));
    assert_eq!(r["error"]["code"], -32602);
    writeln!(ro.tx, "this is not json").unwrap();
    let junk = ro.rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(junk["error"]["code"], -32700);
    let (err, v) = ro.call("read_screen", json!({"target": "no-such-terminal"}));
    assert!(err && v["error"]["code"] == "target_not_found", "{v}");

    // A wait the client cancels.
    let wid = ro.send_req(
        "tools/call",
        json!({"name": "wait_for_text", "arguments": {"target": "work", "text": "never-shown-text", "timeout_s": 60}}),
    );
    std::thread::sleep(Duration::from_millis(600));
    let t0 = Instant::now();
    ro.notify(
        "notifications/cancelled",
        json!({"requestId": wid, "reason": "test"}),
    );
    let r = ro.wait(wid);
    assert!(t0.elapsed() < Duration::from_secs(5), "cancelled promptly");
    assert_eq!(
        r["result"]["structuredContent"]["error"]["code"], "cancelled",
        "{r}"
    );
    drop(ro);

    // Granted: create, input, destructive (not run).
    let mut rw = Mcp::start(&ep, &appdata, &["create", "input", "destructive"]);
    let names = rw.tool_names();
    assert!(
        names.contains(&"kill_terminal".to_string())
            && !names.contains(&"recover_terminal".to_string())
    );
    let (err, v) = rw.call(
        "create_terminal",
        json!({"name": "evil", "command": ["calc.exe"]}),
    );
    assert!(
        err && v["error"]["code"] == "permission_denied",
        "a program needs `run`: {v}"
    );
    let v = rw.ok("create_terminal", json!({"name": "agent"}));
    assert_eq!(v["name"], "agent", "{v}");
    rw.ok(
        "send_text",
        json!({"target": "agent", "text": "echo mcp-%OS%"}),
    );
    let w = rw.ok(
        "wait_for_text",
        json!({"target": "agent", "text": "mcp-Windows_NT", "timeout_s": 20}),
    );
    assert_eq!(w["found"], true, "{w}");
    rw.ok("send_keys", json!({"target": "agent", "keys": ["C-c"]}));
    rw.ok(
        "split_pane",
        json!({"target": "agent", "direction": "horizontal"}),
    );
    // Destructive: a preview until confirmed.
    let v = rw.ok("kill_terminal", json!({"target": "agent"}));
    assert_eq!(v["dry_run"], true, "{v}");
    assert!(v["confirm_needed"].is_string());
    let (_, doc) = run(&["info", "agent", "--endpoint", &ep]);
    assert_eq!(
        doc["data"]["terminal"]["state"], "running",
        "still running after the preview: {doc}"
    );
    rw.ok("kill_terminal", json!({"target": "agent", "confirm": true}));
    let (_, doc) = run(&["info", "agent", "--endpoint", &ep]);
    assert_ne!(doc["data"]["terminal"]["state"], "running", "{doc}");
    let (err, v) = rw.call(
        "send_text",
        json!({"target": "work", "text": "x".repeat(5000)}),
    );
    assert!(err && v["error"]["code"] == "invalid_params", "{v}");
    drop(rw);

    // The audit trail names tools and targets, never the typed text.
    let audit = std::fs::read_to_string(appdata.join("mouv").join("subak").join("mcp-audit.jsonl"))
        .unwrap();
    assert!(
        audit.contains("\"tool\":\"send_text\"") && audit.contains("\"tool\":\"kill_terminal\""),
        "{audit}"
    );
    assert!(
        !audit.contains("mcp-%OS%") && !audit.contains("calc"),
        "{audit}"
    );
    for l in audit.lines() {
        let v: Value = serde_json::from_str(l).unwrap();
        assert!(v["tool"].is_string() && v["ok"].is_boolean(), "{l}");
    }
    drop(host);
    let _ = std::fs::remove_dir_all(&appdata);
}

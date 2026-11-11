//! Step 62 (NOEUL-FR-113, NOEUL-SEC-010, ADR 0076): `noeul mcp serve` against the real instance
//! on the debug build's synthetic desktop (nothing read from the screen):
//!
//! - without grants only the read tools exist; no tool reads the clipboard;
//! - `--allow capture` alone is not enough: Noeul refuses with `capture_not_allowed` until the
//!   user turns on `privacy.mcp_captures`; then the capture is written to the PNG file;
//! - `--allow pin` pins a local image file;
//! - every call is audited without a path.
#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use common::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::time::{Duration, Instant};

struct Mcp {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    n: u64,
}

impl Mcp {
    fn start(sb: &Sandbox, allow: &[&str]) -> Mcp {
        let mut args = vec!["mcp", "serve"];
        for a in allow {
            args.extend(["--allow", a]);
        }
        let mut child = sb
            .cmd(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut m = Mcp { child, out, n: 0 };
        let init = m.rpc("initialize", &json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}));
        assert_eq!(init["result"]["serverInfo"]["name"], "noeul", "{init}");
        m
    }

    fn rpc(&mut self, method: &str, params: &Value) -> Value {
        self.n += 1;
        let msg = json!({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params});
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"))
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        self.rpc("tools/call", &json!({"name": name, "arguments": args}))["result"].clone()
    }

    fn names(&mut self) -> Vec<String> {
        self.rpc("tools/list", &json!({}))["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

#[test]
fn agents_read_freely_but_capture_only_with_both_consents_and_never_the_clipboard() {
    let mut sb = Sandbox::synthetic("mcp", Keys::NoGlobals);
    sb.instance();
    let (_, st) = sb.json(&["status"]);
    let m = st["data"]["desktop"]["monitors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["primary"] == true)
        .unwrap()
        .clone();
    let (x, y) = (
        m["rect"]["x"].as_i64().unwrap(),
        m["rect"]["y"].as_i64().unwrap(),
    );
    let region = json!({"x": x + 10, "y": y + 10, "width": 8, "height": 6});

    let mut reader = Mcp::start(&sb, &[]);
    assert_eq!(
        reader.names(),
        ["noeul_status", "noeul_pin_list", "noeul_history_list"]
    );
    let s = reader.call("noeul_status", &json!({}));
    assert_eq!(s["isError"], false, "{s}");
    let r = reader.rpc(
        "tools/call",
        &json!({"name": "noeul_capture_to_file", "arguments": {}}),
    );
    assert_eq!(r["error"]["code"], -32602, "ungranted: unknown");
    drop(reader);

    let mut agent = Mcp::start(&sb, &["capture,pin"]);
    let names = agent.names();
    assert!(
        names.contains(&"noeul_capture_to_file".to_string())
            && names.contains(&"noeul_pin_file".to_string())
    );
    assert!(names.iter().all(|n| !n.contains("clipboard")));
    let out = sb.temp_dir().join("agent.png");
    let args = json!({"region": region, "output": out.display().to_string()});
    let r = agent.call("noeul_capture_to_file", &args);
    assert_eq!(
        (
            r["isError"].as_bool(),
            r["structuredContent"]["error"]["code"].as_str()
        ),
        (Some(true), Some("capture_not_allowed")),
        "{r}"
    );
    assert!(!out.exists(), "nothing captured without the user's setting");

    let (code, _) = sb.json(&["config", "set", "privacy.mcp_captures", "on"]);
    assert_eq!(code, 0);
    // The instance reloads its settings from the file; the next call succeeds.
    let until = Instant::now() + Duration::from_secs(15);
    let r = loop {
        let r = agent.call("noeul_capture_to_file", &args);
        if r["isError"] == false {
            break r;
        }
        assert!(Instant::now() < until, "never allowed: {r}");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(r["structuredContent"]["file"]["written"], true, "{r}");
    assert!(out.is_file());
    assert!(
        !r.to_string().contains("agent.png"),
        "the path is never echoed"
    );
    let p = agent.call(
        "noeul_pin_file",
        &json!({"path": out.display().to_string()}),
    );
    assert_eq!(p["isError"], false, "{p}");
    let l = agent.call("noeul_pin_list", &json!({}));
    assert_eq!(
        l["structuredContent"]["pins"].as_array().unwrap().len(),
        1,
        "{l}"
    );
    drop(agent);

    let audit = std::fs::read_to_string(
        sb.root
            .join("Local")
            .join("mouv")
            .join("noeul")
            .join("mcp-audit.jsonl"),
    )
    .unwrap();
    let lines: Vec<Value> = audit
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l["tool"] == "noeul_capture_to_file" && l["outcome"] == "capture_not_allowed")
    );
    assert!(
        lines
            .iter()
            .any(|l| l["tool"] == "noeul_capture_to_file" && l["outcome"] == "ok")
    );
    assert!(!audit.contains("agent.png"), "no path in the audit");
    sb.stop_all();
}

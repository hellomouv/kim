//! Step 62 (NOEUL-FR-113, NOEUL-SEC-010, ADR 0076): `noeul mcp serve` — an optional, local MCP
//! adapter over stdio for agents (Claude Code and others).
//!
//! - **Started only by the user's MCP client** (its server configuration); it serves JSON-RPC 2.0
//!   on stdin/stdout, one message per line. No socket, no listener, and it never starts Noeul.
//! - **Read by default**: `noeul_status`, `noeul_pin_list`, `noeul_history_list` (geometry, ids
//!   and times; never pixels, paths or titles).
//! - **Two keys for anything more.** `--allow pin` (in the client's configuration, written by the
//!   user) adds `noeul_pin_file`; `--allow capture` adds `noeul_capture_to_file`, which the
//!   running Noeul still refuses with `capture_not_allowed` unless the user also turned on
//!   `privacy.mcp_captures` — and every capture shows Noeul's notice. An ungranted tool is not even
//!   listed. **No tool reads the clipboard.**
//! - **Audited**: every call is appended to `%LOCALAPPDATA%\mouv\noeul\mcp-audit.jsonl` (time,
//!   tool, outcome) — never a path, a region's pixels or a title.

use crate::{Ctx, Outcome};
use noeul::error::{Code, NoeulError, Result};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{BufRead, Write};

pub const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the user granted beyond reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grant {
    /// Pin a local image file the agent names.
    Pin,
    /// Capture a region or a monitor to a PNG file (also needs `privacy.mcp_captures`).
    Capture,
}

impl Grant {
    pub fn parse(s: &str) -> Option<Grant> {
        match s {
            "pin" => Some(Grant::Pin),
            "capture" => Some(Grant::Capture),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Grant::Pin => "pin",
            Grant::Capture => "capture",
        }
    }
}

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub grant: Option<Grant>,
    pub schema: Value,
    pub read_only: bool,
}

fn rect_schema() -> Value {
    json!({"type": "object", "required": ["x", "y", "width", "height"], "additionalProperties": false,
        "properties": {"x": {"type": "integer"}, "y": {"type": "integer"},
            "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1}}})
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "noeul_status",
            title: "Noeul status",
            description: "Whether Noeul runs, its pins, monitors and shortcut registrations (no pixels, titles or paths).",
            grant: None,
            schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            read_only: true,
        },
        Tool {
            name: "noeul_pin_list",
            title: "List pins",
            description: "The open pins: ids, rectangles, zoom, visibility, group (never pixels).",
            grant: None,
            schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            read_only: true,
        },
        Tool {
            name: "noeul_history_list",
            title: "List kept captures",
            description: "Noeul's capture history: numbers, rectangles, times, how each ended (never pixels).",
            grant: None,
            schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            read_only: true,
        },
        Tool {
            name: "noeul_pin_file",
            title: "Pin an image file",
            description: "Pin a local image file (PNG, JPEG, GIF, BMP, ICO, TIFF, TGA; an absolute path on a local drive) on the user's screen, without focus.",
            grant: Some(Grant::Pin),
            schema: json!({"type": "object", "required": ["path"], "additionalProperties": false,
                "properties": {"path": {"type": "string", "description": "Absolute path, e.g. C:\\\\Users\\\\me\\\\Pictures\\\\a.png"}}}),
            read_only: false,
        },
        Tool {
            name: "noeul_capture_to_file",
            title: "Capture to a PNG file",
            description: "Capture a region or a whole monitor of the user's screen to a PNG file. Refused unless the user turned on agent captures in Noeul (privacy.mcp_captures); every capture shows a notice on the user's screen. Ask the user before capturing: screens can show private information.",
            grant: Some(Grant::Capture),
            schema: json!({"type": "object", "required": ["output"], "additionalProperties": false,
                "properties": {
                    "region": rect_schema(),
                    "monitor": {"description": "Instead of region: 1 is the leftmost, or primary, or pointer.",
                        "oneOf": [{"type": "integer", "minimum": 1}, {"enum": ["primary", "pointer"]}]},
                    "output": {"type": "string", "description": "Absolute .png path on a local drive."},
                    "replace": {"type": "boolean", "description": "Write over an existing file."}}}),
            read_only: false,
        },
    ]
}

pub fn offered(grants: &BTreeSet<Grant>) -> Vec<Tool> {
    tools()
        .into_iter()
        .filter(|t| t.grant.is_none_or(|g| grants.contains(&g)))
        .collect()
}

fn tool_json(t: &Tool) -> Value {
    json!({"name": t.name, "title": t.title, "description": t.description, "inputSchema": t.schema,
        "annotations": {"readOnlyHint": t.read_only, "destructiveHint": false, "openWorldHint": false}})
}

pub fn grants(allow: &[String]) -> Result<BTreeSet<Grant>> {
    allow
        .iter()
        .flat_map(|a| a.split(','))
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            Grant::parse(s.trim()).ok_or_else(|| {
                NoeulError::new(Code::Usage, format!("`{s}` is not a grant: pin or capture"))
            })
        })
        .collect()
}

/// `noeul mcp tools`: the tools `serve` would offer with these grants (read-only).
pub fn tools_cmd(allow: &[String]) -> Result<Outcome> {
    let g = grants(allow)?;
    let list: Vec<Value> = offered(&g).iter().map(tool_json).collect();
    let mut human = String::new();
    for t in offered(&g) {
        human.push_str(&format!(
            "{:<24} {}\n",
            t.name,
            t.grant.map_or("read", Grant::word)
        ));
    }
    Ok(Outcome::new(
        json!({"tools": list, "grants": g.iter().map(|g| g.word()).collect::<Vec<_>>()}),
        human,
    ))
}

/// One tool call: the IPC method and its parameters, or a tool error.
pub fn request_for(name: &str, args: &Value) -> std::result::Result<(&'static str, Value), String> {
    match name {
        "noeul_status" => Ok(("status", json!({}))),
        "noeul_pin_list" => Ok(("pin.list", json!({}))),
        "noeul_history_list" => Ok(("history.list", json!({}))),
        "noeul_pin_file" => {
            let path = args["path"].as_str().ok_or("`path` is required")?;
            Ok((
                "pin.create",
                json!({"source": "file", "path": path, "via": "mcp"}),
            ))
        }
        "noeul_capture_to_file" => {
            let output = args["output"].as_str().ok_or("`output` is required")?;
            let mut p = json!({"output": output, "replace": args["replace"] == true, "via": "mcp"});
            match (args.get("region"), args.get("monitor")) {
                (Some(r), None) => p["region"] = r.clone(),
                (None, Some(m)) => p["monitor"] = m.clone(),
                _ => return Err("give exactly one of `region` or `monitor`".into()),
            }
            Ok(("capture", p))
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// A tool's IPC request: the method and parameters in, the answer or `(code, message)` out.
pub type Call<'a> = dyn FnMut(&str, Value) -> std::result::Result<Value, (String, String)> + 'a;

fn rpc_result(id: &Value, result: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The answer to one message (`None` for notifications). `call` performs a tool's IPC request.
pub fn respond(
    grants: &BTreeSet<Grant>,
    msg: &Value,
    call: &mut Call<'_>,
    audit: &mut dyn FnMut(&str, &str),
) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg["method"].as_str().unwrap_or("");
    if msg["jsonrpc"] != "2.0" || method.is_empty() {
        return Some(rpc_error(&id, -32600, "not a JSON-RPC 2.0 request"));
    }
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let v = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .copied()
                .unwrap_or(VERSIONS[0]);
            let g: Vec<&str> = grants.iter().map(|g| g.word()).collect();
            rpc_result(
                &id,
                &json!({
                    "protocolVersion": v,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "noeul", "title": "Noeul screenshots and pins", "version": noeul::VERSION},
                    "instructions": format!(
                        "Noeul is the user's screenshot and pin tool on Windows. Reading is always available. Granted: {}. Captures need the user's consent twice (the grant and privacy.mcp_captures in Noeul) and always show a notice; ask the user before capturing. No tool reads the clipboard.",
                        if g.is_empty() { "nothing beyond reading".to_string() } else { g.join(", ") }
                    ),
                }),
            )
        }
        "ping" => rpc_result(&id, &json!({})),
        "tools/list" => rpc_result(
            &id,
            &json!({"tools": offered(grants).iter().map(tool_json).collect::<Vec<_>>()}),
        ),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let Some(tool) = offered(grants).into_iter().find(|t| t.name == name) else {
                audit(name, "not_offered");
                return Some(rpc_error(&id, -32602, &format!("unknown tool `{name}`")));
            };
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let r = request_for(tool.name, &args)
                .map_err(|e| ("usage".to_string(), e))
                .and_then(|(m, p)| call(m, p));
            let (value, is_error) = match r {
                Ok(v) => {
                    audit(tool.name, "ok");
                    (v, false)
                }
                Err((code, message)) => {
                    audit(tool.name, &code);
                    (json!({"error": {"code": code, "message": message}}), true)
                }
            };
            let text = serde_json::to_string_pretty(&value).unwrap_or_default();
            rpc_result(
                &id,
                &json!({"content": [{"type": "text", "text": text}], "structuredContent": value, "isError": is_error}),
            )
        }
        _ => rpc_error(&id, -32601, &format!("method `{method}` is not supported")),
    })
}

/// `noeul mcp serve [--allow pin|capture]…`: serves until stdin ends. Stdout carries only the
/// JSON-RPC messages (no envelope is printed afterwards).
pub fn serve(ctx: &Ctx, allow: &[String]) -> Result<()> {
    let g = grants(allow)?;
    let audit_file = noeul::paths::Paths::from_env()
        .ok()
        .map(|p| p.data_dir.join("mcp-audit.jsonl"));
    let mut audit = |tool: &str, outcome: &str| {
        if let Some(f) = &audit_file {
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let line = json!({"at": noeul::util::rfc3339(std::time::SystemTime::now()), "tool": tool, "outcome": outcome});
            if let Ok(mut fh) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(f)
            {
                let _ = writeln!(fh, "{line}");
            }
        }
    };
    let mut call = |method: &str, params: Value| -> std::result::Result<Value, (String, String)> {
        #[cfg(windows)]
        {
            match crate::instance::connect(ctx) {
                Ok(Some(mut s)) => s
                    .request(method, params)
                    .map_err(|e| (e.code.as_str().to_string(), e.message)),
                Ok(None) if method == "status" => Ok(json!({"running": false})),
                Ok(None) => Err((
                    "noeul_not_running".into(),
                    "Noeul is not running; the user starts it (an agent never does)".into(),
                )),
                Err(e) => Err((e.code.as_str().to_string(), e.message)),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (method, params);
            Err((
                "unsupported_platform".into(),
                "Noeul runs on Windows".into(),
            ))
        }
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => respond(&g, &msg, &mut call, &mut audit),
            Err(_) => Some(rpc_error(&Value::Null, -32700, "not JSON")),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
    let _ = ctx;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(g: &BTreeSet<Grant>, msg: &Value, calls: &mut Vec<(String, Value)>) -> Value {
        let mut call = |m: &str, p: Value| {
            calls.push((m.to_string(), p));
            Ok(json!({"ok": true}))
        };
        let mut audit = |_: &str, _: &str| {};
        respond(g, msg, &mut call, &mut audit).unwrap()
    }

    /// NOEUL-FR-113: reading only without grants; ungranted tools are not listed nor callable;
    /// no clipboard tool at all.
    #[test]
    fn only_granted_tools_exist_and_none_reads_the_clipboard() {
        let none = BTreeSet::new();
        let mut calls = Vec::new();
        let l = rpc(
            &none,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            &mut calls,
        );
        let names: Vec<&str> = l["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["noeul_status", "noeul_pin_list", "noeul_history_list"]
        );
        let r = rpc(
            &none,
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "noeul_capture_to_file", "arguments": {"output": "C:\\\\a.png", "monitor": 1}}}),
            &mut calls,
        );
        assert_eq!(r["error"]["code"], -32602);
        assert!(calls.is_empty(), "nothing reached Noeul");
        assert!(tools().iter().all(|t| !t.name.contains("clipboard")
            && !t.description.to_lowercase().contains("reads the clipboard")));
        let all = grants(&["pin,capture".into()]).unwrap();
        assert_eq!(offered(&all).len(), 5);
        assert!(grants(&["clipboard".into()]).is_err());
    }

    /// Every agent request is marked `via: mcp`, so Noeul applies `privacy.mcp_captures`.
    #[test]
    fn calls_are_marked_as_agent_requests() {
        let g = grants(&["capture".into(), "pin".into()]).unwrap();
        let mut calls = Vec::new();
        rpc(
            &g,
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "noeul_capture_to_file", "arguments": {"output": "C:\\\\x.png", "region": {"x": 0, "y": 0, "width": 4, "height": 4}}}}),
            &mut calls,
        );
        assert_eq!(calls[0].0, "capture");
        assert_eq!(
            (calls[0].1["via"].as_str(), calls[0].1["pin"].as_bool()),
            (Some("mcp"), None)
        );
        rpc(
            &g,
            &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "noeul_pin_file", "arguments": {"path": "C:\\\\a.png"}}}),
            &mut calls,
        );
        assert_eq!(
            (calls[1].0.as_str(), calls[1].1["via"].as_str()),
            ("pin.create", Some("mcp"))
        );
        let bad = rpc(
            &g,
            &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": {"name": "noeul_capture_to_file", "arguments": {"output": "C:\\\\x.png"}}}),
            &mut calls,
        );
        assert_eq!(bad["result"]["isError"], true);
        assert_eq!(calls.len(), 2, "a bad call reaches nothing");
        let init = rpc(
            &g,
            &json!({"jsonrpc": "2.0", "id": 6, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
            &mut calls,
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(
            init["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("No tool reads the clipboard")
        );
    }
}

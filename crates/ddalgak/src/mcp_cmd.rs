//! Step 81 (DDALGAK-FR-060, DDALGAK-FR-062, DDALGAK-SEC-006, ADR 0082): Ddalgak for agents and
//! for other Kim tools.
//!
//! **`ddalgak mcp serve [--allow rules|modes|profiles|engine]…`** — an optional, local MCP adapter
//! over stdio (JSON-RPC 2.0, one message per line), started only by the user's MCP client from
//! the configuration the user wrote. No socket, no listener; it never starts the app.
//!
//! - **Reading is always offered**: status, rules (titles, triggers, step types — never typed
//!   text, paths or URLs), the rule check, modes, profiles, and `explain` (which rule a key would
//!   fire in a context; nothing runs).
//! - **Changes need the user's grant** in the client configuration, one area at a time: `rules`
//!   (turn rules on or off — a rule that needs consent stays off, because agents never grant
//!   consent), `modes` (switch or reset the mode), `profiles` (switch the active profile),
//!   `engine` (pause or resume). An ungranted tool is not even listed.
//! - **Never**: run a rule or macro, type, open a program, grant consent, write rule content,
//!   read the clipboard or a window title.
//! - Every tool is exactly one fixed `ddalgak` command line run without a window (`--json
//!   --no-input`), so the adapter can do nothing the CLI could not; arguments are validated
//!   (IDs, bounded texts, nothing that looks like an option). Every call is appended to
//!   `%LOCALAPPDATA%\mouv\ddalgak\mcp-audit.jsonl` (time, tool, outcome).
//!
//! **`ddalgak interop invoke --json --no-input`** — Ddalgak as a provider of Kim interop
//! capabilities (docs/INTEROP.md): one `kim.interop.request` on stdin, one capability, the same
//! command lines. `keys.explain`, `rules.list` and `mode.status` read; `mode.set` writes (Kim
//! asks the user's consent for writes).

use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};

pub const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the user granted beyond reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grant {
    Rules,
    Modes,
    Profiles,
    Engine,
}

impl Grant {
    pub fn parse(s: &str) -> Option<Grant> {
        match s {
            "rules" => Some(Grant::Rules),
            "modes" => Some(Grant::Modes),
            "profiles" => Some(Grant::Profiles),
            "engine" => Some(Grant::Engine),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Grant::Rules => "rules",
            Grant::Modes => "modes",
            Grant::Profiles => "profiles",
            Grant::Engine => "engine",
        }
    }
}

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub grant: Option<Grant>,
    pub schema: Value,
}

fn obj(props: &Value, required: &[&str]) -> Value {
    json!({"type": "object", "additionalProperties": false, "properties": props, "required": required})
}

const ID: &str = "^[a-z0-9][a-z0-9-]{0,62}$";

pub fn tools() -> Vec<Tool> {
    let none = obj(&json!({}), &[]);
    vec![
        Tool {
            name: "ddalgak_status",
            title: "Ddalgak status",
            grant: None,
            schema: none.clone(),
            description: "The engine's state, the active profile and mode, rule counts and the recent activity (rule IDs and outcome codes only).",
        },
        Tool {
            name: "ddalgak_rule_list",
            title: "List rules",
            grant: None,
            schema: obj(
                &json!({"profile": {"type": "string", "pattern": ID}, "mode": {"type": "string", "pattern": ID}}),
                &[],
            ),
            description: "Every rule with its title, trigger, step types and state (never typed text, paths or URLs).",
        },
        Tool {
            name: "ddalgak_rule_show",
            title: "Show a rule",
            grant: None,
            schema: obj(&json!({"id": {"type": "string", "pattern": ID}}), &["id"]),
            description: "One rule as stored, with snippet text and launch targets replaced by counts.",
        },
        Tool {
            name: "ddalgak_rule_check",
            title: "Check the rules",
            grant: None,
            schema: none.clone(),
            description: "Validates the rules file and lists problems with their non-destructive offers.",
        },
        Tool {
            name: "ddalgak_mode_list",
            title: "List modes",
            grant: None,
            schema: none.clone(),
            description: "The active profile's modes and which one is active.",
        },
        Tool {
            name: "ddalgak_profile_list",
            title: "List profiles",
            grant: None,
            schema: none,
            description: "Every profile with its rule count and which one is active.",
        },
        Tool {
            name: "ddalgak_explain",
            title: "Explain a key",
            grant: None,
            schema: obj(
                &json!({"keys": {"type": "string", "minLength": 1, "maxLength": 64},
                "context": {"type": "object", "description": "mode, engine, focus, app {exe, window_title}, ime_composing"}}),
                &["keys"],
            ),
            description: "Which rule a key would fire in a context, every candidate and why, and a preview of each step. Nothing is pressed, typed or opened.",
        },
        Tool {
            name: "ddalgak_rule_enable",
            title: "Turn rules on",
            grant: Some(Grant::Rules),
            schema: obj(
                &json!({"ids": {"type": "array", "minItems": 1, "maxItems": 50, "items": {"type": "string", "pattern": ID}},
                "dry_run": {"type": "boolean"}}),
                &["ids"],
            ),
            description: "Turns rules on. A rule that needs consent (typing, launching, windows) stays off with consent_required: only the user grants consent.",
        },
        Tool {
            name: "ddalgak_rule_disable",
            title: "Turn rules off",
            grant: Some(Grant::Rules),
            schema: obj(
                &json!({"ids": {"type": "array", "minItems": 1, "maxItems": 50, "items": {"type": "string", "pattern": ID}},
                "dry_run": {"type": "boolean"}}),
                &["ids"],
            ),
            description: "Turns rules off.",
        },
        Tool {
            name: "ddalgak_mode_set",
            title: "Switch the mode",
            grant: Some(Grant::Modes),
            schema: obj(
                &json!({"mode": {"type": "string", "pattern": ID}, "dry_run": {"type": "boolean"}}),
                &["mode"],
            ),
            description: "Switches the running engine to a mode of the active profile.",
        },
        Tool {
            name: "ddalgak_mode_reset",
            title: "Back to the default mode",
            grant: Some(Grant::Modes),
            schema: obj(&json!({"dry_run": {"type": "boolean"}}), &[]),
            description: "Returns the running engine to the profile's default mode.",
        },
        Tool {
            name: "ddalgak_profile_apply",
            title: "Switch the profile",
            grant: Some(Grant::Profiles),
            schema: obj(
                &json!({"id": {"type": "string", "pattern": ID}, "dry_run": {"type": "boolean"}}),
                &["id"],
            ),
            description: "Makes a profile the active one (the running engine switches atomically).",
        },
        Tool {
            name: "ddalgak_pause",
            title: "Pause the engine",
            grant: Some(Grant::Engine),
            schema: obj(&json!({"dry_run": {"type": "boolean"}}), &[]),
            description: "Pauses every rule except engine-control rules.",
        },
        Tool {
            name: "ddalgak_resume",
            title: "Resume the engine",
            grant: Some(Grant::Engine),
            schema: obj(&json!({"dry_run": {"type": "boolean"}}), &[]),
            description: "Resumes a paused engine.",
        },
    ]
}

/// The tools a set of grants offers.
pub fn offered(grants: &[Grant]) -> Vec<Tool> {
    tools()
        .into_iter()
        .filter(|t| t.grant.is_none_or(|g| grants.contains(&g)))
        .collect()
}

pub fn grants(allow: &[String]) -> Result<Vec<Grant>, String> {
    let mut out = Vec::new();
    for a in allow {
        let g = Grant::parse(a)
            .ok_or_else(|| format!("--allow takes rules, modes, profiles or engine, not `{a}`"))?;
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out.sort();
    Ok(out)
}

fn id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The `ddalgak` command line of a tool call (validated; nothing an agent passes becomes an option).
pub fn argv(name: &str, a: &Value) -> Result<Vec<String>, String> {
    let o = a.as_object().cloned().unwrap_or_default();
    let allowed: &[&str] = match name {
        "ddalgak_rule_list" => &["profile", "mode"],
        "ddalgak_rule_show" | "ddalgak_profile_apply" => &["id", "dry_run"],
        "ddalgak_explain" => &["keys", "context"],
        "ddalgak_rule_enable" | "ddalgak_rule_disable" => &["ids", "dry_run"],
        "ddalgak_mode_set" => &["mode", "dry_run"],
        _ => &["dry_run"],
    };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("unknown argument `{k}`"));
    }
    let id = |k: &str| -> Result<String, String> {
        let v = o
            .get(k)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("`{k}` is required"))?;
        if id_ok(v) {
            Ok(v.to_string())
        } else {
            Err(format!("`{k}` is not an ID"))
        }
    };
    let s = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
    let mut args = match name {
        "ddalgak_status" => s(&["status"]),
        "ddalgak_rule_list" => {
            let mut v = s(&["rule", "list"]);
            for k in ["profile", "mode"] {
                if o.contains_key(k) {
                    v.push(format!("--{k}"));
                    v.push(id(k)?);
                }
            }
            v
        }
        "ddalgak_rule_show" => vec!["rule".into(), "show".into(), id("id")?],
        "ddalgak_rule_check" => s(&["rule", "check"]),
        "ddalgak_mode_list" => s(&["mode", "list"]),
        "ddalgak_profile_list" => s(&["profile", "list"]),
        "ddalgak_explain" => {
            let keys = o
                .get("keys")
                .and_then(Value::as_str)
                .ok_or("`keys` is required")?;
            if keys.is_empty()
                || keys.len() > 64
                || keys.starts_with('-')
                || keys.chars().any(char::is_control)
            {
                return Err("`keys` is a key or chord like Ctrl+Shift+K".into());
            }
            let mut v = vec!["explain".to_string(), keys.to_string()];
            if let Some(c) = o.get("context") {
                if !c.is_object() {
                    return Err("`context` is an object".into());
                }
                v.push("--context".into());
                v.push(c.to_string());
            }
            v
        }
        "ddalgak_rule_enable" | "ddalgak_rule_disable" => {
            let ids = o
                .get("ids")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty() && a.len() <= 50)
                .ok_or("`ids` lists 1–50 rule IDs")?;
            let mut v = vec![
                "rule".to_string(),
                if name.ends_with("enable") {
                    "enable"
                } else {
                    "disable"
                }
                .to_string(),
            ];
            for i in ids {
                let i = i
                    .as_str()
                    .filter(|i| id_ok(i))
                    .ok_or("`ids` are rule IDs")?;
                v.push(i.to_string());
            }
            v
        }
        "ddalgak_mode_set" => vec!["mode".into(), "set".into(), id("mode")?],
        "ddalgak_mode_reset" => s(&["mode", "reset"]),
        "ddalgak_profile_apply" => vec!["profile".into(), "apply".into(), id("id")?],
        "ddalgak_pause" => s(&["pause"]),
        "ddalgak_resume" => s(&["resume"]),
        _ => return Err(format!("unknown tool `{name}`")),
    };
    if o.get("dry_run").and_then(Value::as_bool) == Some(true) {
        args.push("--dry-run".into());
    }
    Ok(args)
}

/// Runs one fixed `ddalgak` command line, hidden; its JSON envelope.
pub type Runner<'a> = dyn FnMut(&[String]) -> Value + 'a;

/// This program, run without a window (`--json --no-input`), optionally on another endpoint.
pub fn run_self(args: &[String], endpoint: Option<&str>) -> Value {
    let Ok(exe) = std::env::current_exe() else {
        return json!({"ok": false, "error": {"code": "internal", "message": "cannot find ddalgak.exe"}});
    };
    let mut c = std::process::Command::new(exe);
    c.args(args).args(["--json", "--no-input"]);
    let takes_endpoint = matches!(
        args.first().map(String::as_str),
        Some("status" | "mode" | "pause" | "resume")
    ) && !(args.first().map(String::as_str) == Some("mode")
        && args.get(1).map(String::as_str) == Some("list"));
    if let (Some(e), true) = (endpoint, takes_endpoint) {
        c.args(["--endpoint", e]);
    }
    c.stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    match c.output() {
        Ok(o) => serde_json::from_slice(&o.stdout).unwrap_or_else(
            |_| json!({"ok": false, "error": {"code": "internal", "message": "no JSON answer"}}),
        ),
        Err(e) => json!({"ok": false, "error": {"code": "internal", "message": e.to_string()}}),
    }
}

fn rpc_result(id: &Value, result: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_json(t: &Tool) -> Value {
    json!({"name": t.name, "title": t.title, "description": t.description, "inputSchema": t.schema,
        "annotations": {"readOnlyHint": t.grant.is_none(), "destructiveHint": false, "openWorldHint": false}})
}

/// One JSON-RPC message; `None` for a notification.
pub fn respond(
    grants: &[Grant],
    msg: &Value,
    run: &mut Runner<'_>,
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
                    "serverInfo": {"name": "ddalgak", "title": "Ddalgak automation rules", "version": ddalgak::VERSION},
                    "instructions": format!(
                        "Ddalgak is the user's key remapping and automation engine on Windows. Reading and explaining are always available. Granted changes: {}. You can never run a rule or macro, type, open programs, grant consent or read typed text, the clipboard or window titles; ask the user to do those.",
                        if g.is_empty() { "none".to_string() } else { g.join(", ") }
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
            let (value, is_error) = match argv(tool.name, &args) {
                Err(e) => {
                    audit(tool.name, "usage");
                    (json!({"error": {"code": "usage", "message": e}}), true)
                }
                Ok(argv) => {
                    let v = run(&argv);
                    let ok = v["ok"] == true;
                    let code = if ok {
                        "ok".to_string()
                    } else {
                        v["error"]["code"]
                            .as_str()
                            .unwrap_or("internal")
                            .to_string()
                    };
                    audit(tool.name, &code);
                    let value = if ok {
                        v["data"].clone()
                    } else {
                        json!({"error": v["error"]})
                    };
                    (value, !ok)
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

fn audit_file() -> Option<std::path::PathBuf> {
    ddalgak::paths::Paths::from_env()
        .ok()
        .map(|p| p.local.join("mcp-audit.jsonl"))
}

/// `ddalgak mcp serve`: serves until stdin ends. Stdout carries only JSON-RPC messages.
pub fn serve(allow: &[String], endpoint: Option<&str>) -> Result<(), String> {
    let g = grants(allow)?;
    let file = audit_file();
    let mut audit = |tool: &str, outcome: &str| {
        if let Some(f) = &file {
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let line =
                json!({"at": ddalgak::time::now_rfc3339(), "tool": tool, "outcome": outcome});
            if let Ok(mut fh) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(f)
            {
                let _ = writeln!(fh, "{line}");
            }
        }
    };
    let mut run = |args: &[String]| run_self(args, endpoint);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => respond(&g, &msg, &mut run, &mut audit),
            Err(_) => Some(rpc_error(&Value::Null, -32700, "not JSON")),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
    Ok(())
}

/// `ddalgak mcp tools`: what the adapter offers with these grants.
pub fn tools_json(allow: &[String]) -> Result<Value, String> {
    let g = grants(allow)?;
    Ok(json!({
        "grants": g.iter().map(|g| g.word()).collect::<Vec<_>>(),
        "tools": offered(&g).iter().map(|t| json!({"name": t.name, "title": t.title, "read_only": t.grant.is_none(),
            "grant": t.grant.map(Grant::word)})).collect::<Vec<_>>(),
        "audit": audit_file().map(|p| p.display().to_string()),
    }))
}

/// Kim interop capabilities: (id, version, effect, tool).
pub const PROVIDES: [(&str, &str, &str, &str); 4] = [
    ("keys.explain", "1.0.0", "read", "ddalgak_explain"),
    ("rules.list", "1.0.0", "read", "ddalgak_rule_list"),
    ("mode.status", "1.0.0", "read", "ddalgak_mode_list"),
    ("mode.set", "1.0.0", "write", "ddalgak_mode_set"),
];

/// Checks a `kim.interop.request`; returns (capability's tool, params, dry_run) or (code, message).
pub fn parse_request(text: &str) -> Result<(&'static str, Value, bool), (&'static str, String)> {
    if text.len() > 64 * 1024 {
        return Err(("usage", "the request is larger than 64 KiB".into()));
    }
    let req: Value = serde_json::from_str(text)
        .map_err(|e| ("usage", format!("the request on stdin is not JSON: {e}")))?;
    if req["schema"] != "kim.interop.request" || req["schema_version"] != 1 {
        return Err((
            "tool_incompatible",
            "not a kim.interop.request version 1".into(),
        ));
    }
    let cap = req["capability"].as_str().unwrap_or_default();
    let Some((_, version, _, tool)) = PROVIDES.iter().find(|(id, ..)| *id == cap) else {
        return Err((
            "capability_missing",
            format!("Ddalgak does not provide `{cap}`"),
        ));
    };
    let major = version.split('.').next().unwrap_or("1");
    if req["interface"].as_str() != Some(major) {
        return Err((
            "tool_incompatible",
            format!("Ddalgak speaks `{cap}` interface {major}"),
        ));
    }
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(("usage", "`params` must be an object".into()));
    }
    Ok((tool, params, req["dry_run"] == true))
}

/// `ddalgak interop invoke`: reads the request from stdin and answers with the command's envelope.
pub fn interop_invoke(
    force_dry_run: bool,
    endpoint: Option<&str>,
) -> Result<Value, (String, String)> {
    let mut text = String::new();
    std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| ("io".to_string(), format!("cannot read the request: {e}")))?;
    let (tool, mut params, dry) = parse_request(&text).map_err(|(c, m)| (c.to_string(), m))?;
    if (dry || force_dry_run) && tool == "ddalgak_mode_set" {
        params["dry_run"] = json!(true);
    }
    let args = argv(tool, &params).map_err(|e| ("usage".to_string(), e))?;
    let v = run_self(&args, endpoint);
    if v["ok"] == true {
        Ok(v["data"].clone())
    } else {
        Err((
            v["error"]["code"]
                .as_str()
                .unwrap_or("internal")
                .to_string(),
            v["error"]["message"].as_str().unwrap_or("").to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(
        grants: &[Grant],
        name: &str,
        args: &Value,
    ) -> (Value, Vec<Vec<String>>, Vec<(String, String)>) {
        let mut ran = Vec::new();
        let mut audited = Vec::new();
        let msg = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": args}});
        let r = respond(
            grants,
            &msg,
            &mut |a: &[String]| {
                ran.push(a.to_vec());
                json!({"ok": true, "data": {"ran": a}})
            },
            &mut |t, o| audited.push((t.to_string(), o.to_string())),
        )
        .unwrap();
        (r, ran, audited)
    }

    /// DDALGAK-SEC-010: ungranted changes are not listed and are refused; reads always work.
    #[test]
    fn reading_is_free_and_every_change_needs_its_grant() {
        let names = |g: &[Grant]| offered(g).iter().map(|t| t.name).collect::<Vec<_>>();
        let read = names(&[]);
        assert!(
            read.contains(&"ddalgak_explain")
                && !read
                    .iter()
                    .any(|n| n.contains("enable") || n.contains("pause"))
        );
        assert!(names(&[Grant::Engine]).contains(&"ddalgak_pause"));
        let (r, ran, audited) = call(&[], "ddalgak_pause", &json!({}));
        assert_eq!(r["error"]["code"], -32602);
        assert!(ran.is_empty());
        assert_eq!(
            audited,
            [("ddalgak_pause".to_string(), "not_offered".to_string())]
        );
        let (r, ran, _) = call(&[Grant::Engine], "ddalgak_pause", &json!({"dry_run": true}));
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(ran, [["pause", "--dry-run"]]);
    }

    /// Malformed or hostile arguments never reach the command line.
    #[test]
    fn arguments_are_validated_and_never_become_options() {
        for (name, args) in [
            ("ddalgak_rule_show", json!({"id": "--purge"})),
            ("ddalgak_rule_show", json!({"id": "a b"})),
            ("ddalgak_rule_show", json!({})),
            ("ddalgak_explain", json!({"keys": "--rule"})),
            ("ddalgak_explain", json!({"keys": "F13", "context": "x"})),
            (
                "ddalgak_rule_list",
                json!({"profile": "x", "show_text": true}),
            ),
            ("ddalgak_rule_enable", json!({"ids": []})),
            ("ddalgak_rule_enable", json!({"ids": ["ok", "--allow"]})),
        ] {
            assert!(argv(name, &args).is_err(), "{name} {args}");
        }
        assert_eq!(
            argv(
                "ddalgak_explain",
                &json!({"keys": "Ctrl+Shift+K", "context": {"mode": "nav"}})
            )
            .unwrap(),
            ["explain", "Ctrl+Shift+K", "--context", "{\"mode\":\"nav\"}"]
        );
        assert_eq!(
            argv("ddalgak_rule_enable", &json!({"ids": ["a", "b-2"]})).unwrap(),
            ["rule", "enable", "a", "b-2"]
        );
        let (r, ran, audited) = call(
            &[Grant::Rules],
            "ddalgak_rule_enable",
            &json!({"ids": ["x"], "allow": "input-injection"}),
        );
        assert!(ran.is_empty(), "an agent can never pass --allow");
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(audited[0].1, "usage");
    }

    #[test]
    fn interop_requests_are_checked() {
        let req = |cap: &str, iface: &str| {
            json!({"schema": "kim.interop.request", "schema_version": 1, "capability": cap,
            "interface": iface, "params": {"keys": "F13"}, "dry_run": false})
            .to_string()
        };
        assert_eq!(
            parse_request(&req("keys.explain", "1")).unwrap().0,
            "ddalgak_explain"
        );
        assert_eq!(
            parse_request(&req("rules.delete", "1")).unwrap_err().0,
            "capability_missing"
        );
        assert_eq!(
            parse_request(&req("keys.explain", "2")).unwrap_err().0,
            "tool_incompatible"
        );
        assert_eq!(parse_request("[").unwrap_err().0, "usage");
        assert!(grants(&["rules".into(), "root".into()]).is_err());
    }
}

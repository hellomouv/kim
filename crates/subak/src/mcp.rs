//! Step 37 (ADR 0070): `subak mcp serve` — a Model Context Protocol server over stdio.
//!
//! - **Local only.** The MCP client (Claude Code, another agent) starts `subak mcp serve` and
//!   speaks JSON-RPC 2.0 over its stdin/stdout, one message per line. No socket, no listener.
//! - **Read-only by default.** Discovery tools (Terminals, panes, screens, Claude status, Host
//!   resources, recoverable Terminals) use an IPC connection of kind `mcp`, which the Host
//!   itself limits to read-only methods; screens are always redacted for it.
//! - **Mutations only when granted.** `--allow create|input|run|control|destructive|recover`
//!   (in the client's server configuration, written by the user) adds the matching tools; an
//!   ungranted tool is not even listed. `run` (a program of the agent's choice, recovering with
//!   start commands) is separate from `create` (shells and profiles). Destructive tools only
//!   preview unless called with `confirm: true`.
//! - **Audited.** Every call is appended to `%APPDATA%\mouv\subak\mcp-audit.jsonl` (tool,
//!   target, grant, outcome) — never typed text, screen text or a secret.

use crate::cli::{Args, Cmd};
use crate::client::{Identity, Session};
use crate::output::{CliError, Code};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Protocol versions this server speaks, newest first.
pub const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// The largest text `send_text` types at once.
const MAX_TEXT: usize = 4096;
/// The most screen text one `read_screen` returns.
const MAX_SCREEN: usize = 256 * 1024;
const DEFAULT_SCREEN: usize = 64 * 1024;

/// What the user granted beyond reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Grant {
    /// New Terminals (shells and profiles) and split panes.
    Create,
    /// Typing into panes (which runs whatever a shell is given: as strong as `run`).
    Input,
    /// A program of the agent's choice; recovering with start commands.
    Run,
    /// Rename, detach other clients.
    Control,
    /// Stop, kill, remove.
    Destructive,
    /// Restore saved Terminals.
    Recover,
}

impl Grant {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "create" => Self::Create,
            "input" => Self::Input,
            "run" => Self::Run,
            "control" => Self::Control,
            "destructive" => Self::Destructive,
            "recover" => Self::Recover,
            _ => return None,
        })
    }
    pub fn word(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Input => "input",
            Self::Run => "run",
            Self::Control => "control",
            Self::Destructive => "destructive",
            Self::Recover => "recover",
        }
    }
}

/// One tool.
pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub grant: Option<Grant>,
    pub destructive: bool,
    pub idempotent: bool,
    pub schema: Value,
}

fn obj(props: Value, required: &[&str]) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("type".into(), json!("object"));
    m.insert("properties".into(), props);
    m.insert("required".into(), json!(required));
    m.insert("additionalProperties".into(), json!(false));
    Value::Object(m)
}

fn target_prop() -> Value {
    json!({"type": "string", "minLength": 1, "maxLength": 128, "description": "A Terminal name or id (t-…), a window (w-…) or a pane (p-…)"})
}

/// Every tool this server knows.
pub fn tools() -> Vec<Tool> {
    let t = target_prop;
    vec![
        Tool {
            name: "list_terminals",
            title: "List Terminals",
            description: "The Terminals of the Subak Host: name, id, state, windows and panes. Empty when no Host runs (none is started).",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "terminal_info",
            title: "Terminal details",
            description: "One Terminal: its windows, panes, programs, sizes and state.",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(json!({"target": t()}), &["target"]),
        },
        Tool {
            name: "list_panes",
            title: "List panes",
            description: "The windows and panes of a Terminal (or of one window), with the active one.",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(json!({"target": t()}), &["target"]),
        },
        Tool {
            name: "read_screen",
            title: "Read a screen",
            description: "The text on a pane's screen, optionally with history lines before it. Secret-looking values are always replaced by ***.",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(
                json!({"target": t(), "history_lines": {"type": "integer", "minimum": 0, "maximum": 10000}, "max_bytes": {"type": "integer", "minimum": 1024, "maximum": MAX_SCREEN}}),
                &["target"],
            ),
        },
        Tool {
            name: "wait_for_text",
            title: "Wait for text",
            description: "Waits until a pane's screen shows the text (at most 60 s); cancellable.",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(
                json!({"target": t(), "text": {"type": "string", "minLength": 1, "maxLength": 256}, "timeout_s": {"type": "integer", "minimum": 1, "maximum": 60}}),
                &["target", "text"],
            ),
        },
        Tool {
            name: "claude_status",
            title: "Claude Code status",
            description: "What Claude Code is doing in each pane (working, waiting for input, idle, …), from its hooks.",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(json!({"target": t()}), &[]),
        },
        Tool {
            name: "host_stats",
            title: "Host resources",
            description: "Whether a Host runs, and its memory, handles and counters.",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "list_recoverable",
            title: "Recoverable Terminals",
            description: "Saved Terminals a restore could bring back, and what it could not.",
            grant: None,
            destructive: false,
            idempotent: true,
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "create_terminal",
            title: "Create a Terminal",
            description: "A new Terminal running the default shell or a profile of config.toml. A `command` (a program of your choice) needs the `run` grant.",
            grant: Some(Grant::Create),
            destructive: false,
            idempotent: false,
            schema: obj(
                json!({"name": {"type": "string", "minLength": 1, "maxLength": 64}, "profile": {"type": "string", "maxLength": 64}, "cwd": {"type": "string", "maxLength": 1024}, "command": {"type": "array", "items": {"type": "string", "maxLength": 4096}, "minItems": 1, "maxItems": 64}}),
                &[],
            ),
        },
        Tool {
            name: "split_pane",
            title: "Split a pane",
            description: "A new pane with the default shell next to (horizontal) or below (vertical) a pane.",
            grant: Some(Grant::Create),
            destructive: false,
            idempotent: false,
            schema: obj(
                json!({"target": t(), "direction": {"enum": ["horizontal", "vertical"]}}),
                &["target"],
            ),
        },
        Tool {
            name: "send_text",
            title: "Type text",
            description: "Types text into a pane (as if typed), then Enter unless enter is false. Whatever a shell receives, it runs.",
            grant: Some(Grant::Input),
            destructive: true,
            idempotent: false,
            schema: obj(
                json!({"target": t(), "text": {"type": "string", "maxLength": MAX_TEXT}, "enter": {"type": "boolean"}}),
                &["target", "text"],
            ),
        },
        Tool {
            name: "send_keys",
            title: "Press keys",
            description: "Presses keys in a pane by name (Enter, C-c, Up, F5, …; tmux names).",
            grant: Some(Grant::Input),
            destructive: true,
            idempotent: false,
            schema: obj(
                json!({"target": t(), "keys": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 32}, "minItems": 1, "maxItems": 64}}),
                &["target", "keys"],
            ),
        },
        Tool {
            name: "rename_terminal",
            title: "Rename a Terminal",
            description: "Gives a Terminal a new name.",
            grant: Some(Grant::Control),
            destructive: false,
            idempotent: true,
            schema: obj(
                json!({"target": t(), "name": {"type": "string", "minLength": 1, "maxLength": 64}}),
                &["target", "name"],
            ),
        },
        Tool {
            name: "detach_clients",
            title: "Detach clients",
            description: "Detaches every client attached to a Terminal; the Terminal keeps running.",
            grant: Some(Grant::Control),
            destructive: false,
            idempotent: true,
            schema: obj(json!({"target": t()}), &["target"]),
        },
        Tool {
            name: "stop_terminal",
            title: "Stop a Terminal",
            description: "Asks a Terminal's programs to end (Ctrl+Break, then the close event). Without confirm: true only shows what would happen.",
            grant: Some(Grant::Destructive),
            destructive: true,
            idempotent: true,
            schema: obj(
                json!({"target": t(), "confirm": {"type": "boolean"}}),
                &["target"],
            ),
        },
        Tool {
            name: "kill_terminal",
            title: "Kill a Terminal",
            description: "Ends every process of a Terminal at once. Without confirm: true only shows what would happen.",
            grant: Some(Grant::Destructive),
            destructive: true,
            idempotent: true,
            schema: obj(
                json!({"target": t(), "confirm": {"type": "boolean"}}),
                &["target"],
            ),
        },
        Tool {
            name: "remove_terminal",
            title: "Remove a Terminal",
            description: "Removes a Terminal that runs nothing (force also ends a running one). Without confirm: true only shows what would happen.",
            grant: Some(Grant::Destructive),
            destructive: true,
            idempotent: true,
            schema: obj(
                json!({"target": t(), "force": {"type": "boolean"}, "confirm": {"type": "boolean"}}),
                &["target"],
            ),
        },
        Tool {
            name: "recover_terminal",
            title: "Recover a Terminal",
            description: "Restores a saved Terminal with fresh shells (with_commands re-runs saved start commands and needs the run grant). Without confirm: true only shows the plan.",
            grant: Some(Grant::Recover),
            destructive: true,
            idempotent: false,
            schema: obj(
                json!({"target": t(), "with_commands": {"type": "boolean"}, "claude_resume": {"type": "boolean"}, "confirm": {"type": "boolean"}}),
                &["target"],
            ),
        },
    ]
}

/// The tools a server with `grants` offers.
pub fn offered(grants: &HashSet<Grant>) -> Vec<Tool> {
    tools()
        .into_iter()
        .filter(|t| t.grant.is_none_or(|g| grants.contains(&g)))
        .collect()
}

fn tool_json(t: &Tool) -> Value {
    json!({
        "name": t.name,
        "title": t.title,
        "description": t.description,
        "inputSchema": t.schema,
        "annotations": {
            "title": t.title,
            "readOnlyHint": t.grant.is_none(),
            "destructiveHint": t.destructive,
            "idempotentHint": t.idempotent,
            "openWorldHint": false,
        },
    })
}

/// The grants of `--allow`, checked.
pub fn grants(a: &Args) -> Result<HashSet<Grant>, CliError> {
    a.grants
        .iter()
        .map(|g| {
            Grant::parse(g).ok_or_else(|| {
                CliError::usage(format!(
                    "--allow `{g}` is not one of create, input, run, control, destructive, recover"
                ))
            })
        })
        .collect()
}

/// `subak mcp tools [--allow …]`: what a server with those grants offers (JSON, no Host).
pub fn tools_cmd(a: &Args) -> Result<(Value, String), CliError> {
    let g = grants(a)?;
    let list: Vec<Value> = offered(&g).iter().map(tool_json).collect();
    let mut h = String::new();
    for t in offered(&g) {
        h.push_str(&format!(
            "{:<18} {}{}\n",
            t.name,
            t.grant.map_or("read".to_string(), |g| g.word().to_string()),
            if t.destructive { " (asks)" } else { "" }
        ));
    }
    let mut gl: Vec<&str> = g.iter().map(|g| g.word()).collect();
    gl.sort_unstable();
    Ok((
        json!({"grants": gl, "tools": list, "protocol_versions": VERSIONS}),
        h,
    ))
}

// ---------------------------------------------------------------------------------------------
// The server.

/// An error of a tool call: reported to the model as a tool result with `isError`.
struct ToolErr(Value);

impl From<CliError> for ToolErr {
    fn from(e: CliError) -> Self {
        Self(json!({"code": e.code.as_str(), "message": e.message, "hint": e.hint}))
    }
}

fn bad(msg: impl Into<String>) -> ToolErr {
    ToolErr(json!({"code": "invalid_params", "message": msg.into()}))
}

struct Server {
    endpoint: String,
    grants: HashSet<Grant>,
    cancelled: Arc<Mutex<HashSet<String>>>,
    audit: Option<std::path::PathBuf>,
}

fn str_arg<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str)
}

fn req_str<'a>(args: &'a Value, k: &str) -> Result<&'a str, ToolErr> {
    str_arg(args, k)
        .filter(|s| !s.is_empty() && !s.contains(['\0', '\r', '\n']))
        .ok_or_else(|| bad(format!("`{k}` is required")))
}

fn flag_arg(args: &Value, k: &str) -> bool {
    args.get(k).and_then(Value::as_bool).unwrap_or(false)
}

impl Server {
    /// A read-only connection (IPC kind `mcp`); `None` when no Host runs.
    fn reader(&self) -> Result<Option<Session>, ToolErr> {
        match Session::try_open_as(&self.endpoint, Duration::from_secs(2), Identity::MCP) {
            Ok(s) => Ok(Some(s)),
            Err(f) if f.err.code == Code::HostNotRunning => Ok(None),
            Err(f) => Err(f.err.into()),
        }
    }

    fn read(&self, method: &str, params: &Value) -> Result<Value, ToolErr> {
        let mut s = self.reader()?.ok_or_else(|| {
            ToolErr(json!({"code": "host_not_running", "message": "no Subak Host runs (nothing is started by reading)"}))
        })?;
        Ok(s.request(method, params)?)
    }

    fn base_args(&self) -> Args {
        Args {
            json: true,
            yes: true,
            no_input: true,
            quiet: true,
            endpoint: Some(self.endpoint.clone()),
            ..Args::default()
        }
    }

    fn screen(&self, target: &str, history: u64, limit: usize) -> Result<(String, Value), ToolErr> {
        let start = if history > 0 {
            json!(-i64::try_from(history).unwrap_or(10_000))
        } else {
            json!(0)
        };
        let r = self.read(
            "pane.capture",
            &json!({"target": target, "start": start, "end": Value::Null, "format": "text", "join": true, "redact": true}),
        )?;
        let mut text = r["lines"]
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let mut truncated = r["truncated"] == true;
        if text.len() > limit {
            // Keep the end (the newest lines), on a character boundary.
            let mut cut = text.len() - limit;
            while !text.is_char_boundary(cut) {
                cut += 1;
            }
            text = text[cut..].to_string();
            truncated = true;
        }
        Ok((
            text,
            json!({"terminal_id": r["terminal_id"], "pane_id": r["pane_id"], "redacted": r["redacted"], "truncated": truncated}),
        ))
    }

    fn call(&self, name: &str, args: &Value, id: &str) -> Result<Value, ToolErr> {
        let Some(tool) = tools().into_iter().find(|t| t.name == name) else {
            return Err(bad(format!("no tool `{name}`")));
        };
        if let Some(g) = tool.grant
            && !self.grants.contains(&g)
        {
            return Err(ToolErr(
                json!({"code": "permission_denied", "message": format!("`{name}` needs the `{}` grant (`subak mcp serve --allow {}`)", g.word(), g.word())}),
            ));
        }
        if !args.is_object() && !args.is_null() {
            return Err(bad("arguments must be an object"));
        }
        let mut a = self.base_args();
        let target = str_arg(args, "target").map(String::from);
        if let Some(t) = &target {
            if t.len() > 128 || t.contains(['\0', '\r', '\n']) {
                return Err(bad("`target` is not a Terminal, window or pane"));
            }
            a.positional = vec![t.clone()];
        }
        match name {
            "list_terminals" => Ok(match self.reader()? {
                Some(mut s) => {
                    let mut r = s.request("terminal.list", &json!({"detail": "summary"}))?;
                    r["host_running"] = json!(true);
                    r
                }
                None => json!({"host_running": false, "terminals": []}),
            }),
            "terminal_info" => self.read(
                "terminal.info",
                &json!({"target": req_str(args, "target")?}),
            ),
            "list_panes" => self.read("pane.list", &json!({"target": req_str(args, "target")?})),
            "claude_status" => self.read("claude.status", &json!({"target": target})),
            "host_stats" => Ok(match self.reader()? {
                Some(mut s) => {
                    let mut r = s.request("host.stats", &json!({}))?;
                    r["host_running"] = json!(true);
                    r
                }
                None => json!({"host_running": false}),
            }),
            "list_recoverable" => Ok(match self.reader()? {
                Some(mut s) => s.request("terminal.recoverable", &json!({}))?,
                None => crate::terminal::recover_list(&self.endpoint)?,
            }),
            "read_screen" => {
                let t = req_str(args, "target")?;
                let h = args
                    .get("history_lines")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(10_000);
                let limit = args
                    .get("max_bytes")
                    .and_then(Value::as_u64)
                    .map_or(DEFAULT_SCREEN, |n| usize::try_from(n).unwrap_or(MAX_SCREEN))
                    .clamp(1024, MAX_SCREEN);
                let (text, mut meta) = self.screen(t, h, limit)?;
                meta["text"] = json!(text);
                Ok(meta)
            }
            "wait_for_text" => {
                let t = req_str(args, "target")?;
                let want = str_arg(args, "text")
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| bad("`text` is required"))?;
                let secs = args
                    .get("timeout_s")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .clamp(1, 60);
                let t0 = Instant::now();
                loop {
                    let (text, _) = self.screen(t, 0, MAX_SCREEN)?;
                    if text.contains(want) {
                        return Ok(
                            json!({"found": true, "waited_ms": u64::try_from(t0.elapsed().as_millis()).unwrap_or(0)}),
                        );
                    }
                    if self.cancelled.lock().is_ok_and(|c| c.contains(id)) {
                        return Err(ToolErr(
                            json!({"code": "cancelled", "message": "the client cancelled the wait"}),
                        ));
                    }
                    if t0.elapsed() >= Duration::from_secs(secs) {
                        return Ok(
                            json!({"found": false, "waited_ms": u64::try_from(t0.elapsed().as_millis()).unwrap_or(0)}),
                        );
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
            "create_terminal" => {
                if let Some(n) = str_arg(args, "name") {
                    a.positional = vec![n.to_string()];
                }
                a.profile = str_arg(args, "profile").map(String::from);
                a.cwd = str_arg(args, "cwd").map(String::from);
                if let Some(c) = args.get("command") {
                    if !self.grants.contains(&Grant::Run) {
                        return Err(ToolErr(
                            json!({"code": "permission_denied", "message": "a `command` needs the `run` grant (`--allow run`); without it, create a shell or a profile"}),
                        ));
                    }
                    a.command = c
                        .as_array()
                        .ok_or_else(|| bad("`command` must be a list of strings"))?
                        .iter()
                        .map(|x| x.as_str().map(String::from))
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| bad("`command` must be a list of strings"))?;
                    if a.command.is_empty() || a.command.iter().any(|w| w.contains('\0')) {
                        return Err(bad("`command` must name a program"));
                    }
                }
                Ok(crate::terminal::new(&a, &self.endpoint)?)
            }
            "split_pane" => {
                req_str(args, "target")?;
                a.split = Some(str_arg(args, "direction") == Some("horizontal"));
                Ok(crate::terminal::split(&a, &self.endpoint)?)
            }
            "send_text" => {
                let t = req_str(args, "target")?;
                let text = str_arg(args, "text").ok_or_else(|| bad("`text` is required"))?;
                if text.chars().count() > MAX_TEXT || text.contains('\0') {
                    return Err(bad(format!(
                        "`text` must be at most {MAX_TEXT} characters without NUL"
                    )));
                }
                let mut s = crate::terminal::require(&self.endpoint)?;
                if !text.is_empty() {
                    s.request(
                        "pane.send_keys",
                        &json!({"target": t, "keys": [text], "literal": true}),
                    )?;
                }
                if args.get("enter").and_then(Value::as_bool).unwrap_or(true) {
                    s.request("pane.send_keys", &json!({"target": t, "keys": ["Enter"]}))?;
                }
                Ok(json!({"sent_characters": text.chars().count()}))
            }
            "send_keys" => {
                let t = req_str(args, "target")?;
                let keys: Vec<String> = args
                    .get("keys")
                    .and_then(Value::as_array)
                    .and_then(|k| k.iter().map(|x| x.as_str().map(String::from)).collect())
                    .filter(|k: &Vec<String>| !k.is_empty() && k.len() <= 64)
                    .ok_or_else(|| bad("`keys` must be 1 to 64 key names"))?;
                let mut s = crate::terminal::require(&self.endpoint)?;
                s.request("pane.send_keys", &json!({"target": t, "keys": keys}))?;
                Ok(json!({"sent_keys": keys.len()}))
            }
            "rename_terminal" => {
                req_str(args, "target")?;
                a.positional.push(req_str(args, "name")?.to_string());
                Ok(crate::terminal::rename(&a, &self.endpoint)?)
            }
            "detach_clients" => {
                req_str(args, "target")?;
                Ok(crate::terminal::detach(&a, &self.endpoint)?)
            }
            "stop_terminal" | "kill_terminal" | "remove_terminal" => {
                req_str(args, "target")?;
                a.force = flag_arg(args, "force");
                a.dry_run = !flag_arg(args, "confirm");
                let cmd = match name {
                    "stop_terminal" => Cmd::Stop,
                    "kill_terminal" => Cmd::Kill,
                    _ => Cmd::Rm,
                };
                let mut r = crate::terminal::end(&a, &self.endpoint, cmd)?;
                if a.dry_run {
                    r["confirm_needed"] = json!(
                        "nothing was changed; call again with confirm: true after the user agrees"
                    );
                }
                Ok(r)
            }
            "recover_terminal" => {
                req_str(args, "target")?;
                a.with_commands = flag_arg(args, "with_commands");
                if a.with_commands && !self.grants.contains(&Grant::Run) {
                    return Err(ToolErr(
                        json!({"code": "permission_denied", "message": "with_commands re-runs saved commands and needs the `run` grant"}),
                    ));
                }
                a.claude_resume = flag_arg(args, "claude_resume");
                a.dry_run = !flag_arg(args, "confirm");
                let mut r = crate::terminal::recover(&a, &self.endpoint)?;
                if a.dry_run {
                    r["confirm_needed"] = json!(
                        "nothing was started; call again with confirm: true after the user agrees"
                    );
                }
                Ok(r)
            }
            _ => Err(bad(format!("no tool `{name}`"))),
        }
    }

    /// One line of the audit trail: what was called, never what was typed or shown.
    fn audit(&self, tool: &str, args: &Value, outcome: &Result<Value, ToolErr>) {
        let Some(path) = &self.audit else { return };
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if std::fs::metadata(path).is_ok_and(|m| m.len() > 1024 * 1024) {
            let _ = std::fs::rename(path, path.with_extension("jsonl.1"));
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut keys: Vec<&str> = args
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        keys.sort_unstable();
        let line = json!({
            "ts": ts,
            "pid": std::process::id(),
            "tool": tool,
            "target": str_arg(args, "target"),
            "argument_names": keys,
            "confirm": flag_arg(args, "confirm"),
            "ok": outcome.is_ok(),
            "error": outcome.as_ref().err().map(|e| e.0["code"].clone()),
        });
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn rpc_result(id: &Value, result: Value) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("jsonrpc".into(), json!("2.0"));
    m.insert("id".into(), id.clone());
    m.insert("result".into(), result);
    Value::Object(m)
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The answer to one message (`None` for notifications).
fn respond(srv: &Server, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let Some(id) = id else {
        // Notifications (initialized, cancelled, …) need no answer.
        return None;
    };
    if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || method.is_empty() {
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
            let mut g: Vec<&str> = srv.grants.iter().map(|g| g.word()).collect();
            g.sort_unstable();
            rpc_result(
                &id,
                json!({
                    "protocolVersion": v,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "subak", "title": "Subak terminals", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": format!(
                        "Subak runs Windows terminals (Terminals with windows and panes) in a detached per-user Host. Read tools are always available; screens are redacted. Granted: {}. Destructive tools only preview unless confirm is true: show the preview to the user and ask before confirming. Targets are Terminal names or ids (t-…), windows (w-…) or panes (p-…).",
                        if g.is_empty() { "nothing beyond reading".to_string() } else { g.join(", ") }
                    ),
                }),
            )
        }
        "ping" => rpc_result(&id, json!({})),
        "tools/list" => rpc_result(
            &id,
            json!({"tools": offered(&srv.grants).iter().map(tool_json).collect::<Vec<_>>()}),
        ),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            if !tools().iter().any(|t| t.name == name) {
                return Some(rpc_error(&id, -32602, &format!("unknown tool `{name}`")));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let key = id.to_string();
            let r = srv.call(name, &args, &key);
            srv.audit(name, &args, &r);
            if let Ok(mut c) = srv.cancelled.lock() {
                c.remove(&key);
            }
            let (value, is_error) = match r {
                Ok(v) => (v, false),
                Err(ToolErr(e)) => (json!({"error": e}), true),
            };
            let text = if let Some(t) = value.get("text").and_then(Value::as_str) {
                t.to_string()
            } else {
                serde_json::to_string_pretty(&value).unwrap_or_default()
            };
            rpc_result(
                &id,
                json!({"content": [{"type": "text", "text": text}], "structuredContent": value, "isError": is_error}),
            )
        }
        _ => rpc_error(&id, -32601, &format!("method `{method}` is not supported")),
    })
}

/// `subak mcp serve [--allow <grant>]…`: serves MCP on stdin/stdout until stdin ends.
pub fn serve(a: &Args, endpoint: &str) -> Result<(), CliError> {
    let srv = Arc::new(Server {
        endpoint: endpoint.to_string(),
        grants: grants(a)?,
        cancelled: Arc::new(Mutex::new(HashSet::new())),
        audit: crate::config::config_dir().map(|d| d.join("mcp-audit.jsonl")),
    });
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let send = |v: &Value| {
        if let Ok(mut o) = out.lock() {
            let _ = writeln!(o, "{v}");
            let _ = o.flush();
        }
    };
    // The reader thread sees cancellations while a tool runs; requests run in order.
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let cancelled = srv.cancelled.clone();
    let reader = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            if line.len() > 1024 * 1024 {
                let _ = tx.send(json!({"__too_long": true}));
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(v) if v["method"] == "notifications/cancelled" => {
                    if let Ok(mut c) = cancelled.lock() {
                        c.insert(v["params"]["requestId"].to_string());
                    }
                }
                Ok(v) => {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(json!({"__parse_error": true}));
                }
            }
        }
    });
    for msg in rx {
        if msg.get("__parse_error").is_some() {
            send(&rpc_error(&Value::Null, -32700, "not valid JSON"));
            continue;
        }
        if msg.get("__too_long").is_some() {
            send(&rpc_error(
                &Value::Null,
                -32600,
                "a message larger than 1 MiB",
            ));
            continue;
        }
        if msg.is_array() {
            send(&rpc_error(
                &Value::Null,
                -32600,
                "batches are not supported",
            ));
            continue;
        }
        if let Some(r) = respond(&srv, &msg) {
            send(&r);
        }
    }
    let _ = reader.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srv(grants: &[Grant]) -> Server {
        Server {
            endpoint: format!("subak-mcp-unit-{}", std::process::id()),
            grants: grants.iter().copied().collect(),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            audit: None,
        }
    }

    #[test]
    fn only_granted_tools_are_offered_and_read_only_ones_say_so() {
        let names = |g: &[Grant]| -> Vec<&'static str> {
            offered(&g.iter().copied().collect())
                .iter()
                .map(|t| t.name)
                .collect()
        };
        let read = names(&[]);
        assert!(read.contains(&"read_screen") && read.contains(&"list_terminals"));
        assert!(
            !read
                .iter()
                .any(|n| ["create_terminal", "send_text", "kill_terminal"].contains(n))
        );
        for t in offered(&HashSet::new()) {
            assert_eq!(
                tool_json(&t)["annotations"]["readOnlyHint"],
                true,
                "{}",
                t.name
            );
        }
        let more = names(&[Grant::Input, Grant::Destructive]);
        assert!(more.contains(&"send_text") && more.contains(&"kill_terminal"));
        assert!(!more.contains(&"create_terminal"));
        let kill = tools()
            .into_iter()
            .find(|t| t.name == "kill_terminal")
            .unwrap();
        assert_eq!(tool_json(&kill)["annotations"]["destructiveHint"], true);
        assert!(Grant::parse("everything").is_none());
    }

    #[test]
    fn the_protocol_handshake_and_refusals() {
        let s = srv(&[]);
        let r = respond(&s, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}})).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], false);
        let r = respond(&s, &json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}})).unwrap();
        assert_eq!(r["result"]["protocolVersion"], VERSIONS[0]);
        assert!(
            respond(
                &s,
                &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
            )
            .is_none()
        );
        let r = respond(
            &s,
            &json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}),
        )
        .unwrap();
        assert_eq!(r["error"]["code"], -32601);
        let r = respond(&s, &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "shell_exec", "arguments": {}}})).unwrap();
        assert_eq!(r["error"]["code"], -32602);
        // A known tool that was not granted: a tool error the model can read, nothing run.
        let r = respond(&s, &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "send_text", "arguments": {"target": "x", "text": "calc"}}})).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(
            r["result"]["structuredContent"]["error"]["code"],
            "permission_denied"
        );
        // Without a Host, reading says so and starts nothing.
        let r = respond(&s, &json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "list_terminals"}})).unwrap();
        assert_eq!(
            r["result"]["structuredContent"]["host_running"], false,
            "{r}"
        );
        let r = respond(&s, &json!({"jsonrpc": "2.0", "id": 7, "method": "ping"})).unwrap();
        assert_eq!(r["result"], json!({}));
        // create with a program needs `run` even when `create` is granted.
        let c = srv(&[Grant::Create]);
        let r = respond(&c, &json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": "create_terminal", "arguments": {"command": ["calc.exe"]}}})).unwrap();
        assert_eq!(
            r["result"]["structuredContent"]["error"]["code"],
            "permission_denied"
        );
    }
}

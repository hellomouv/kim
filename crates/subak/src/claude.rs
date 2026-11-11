//! Step 27 (ADR 0010, ADR 0061): Claude Code session events.
//!
//! - **Hooks, opt-in.** `subak claude hooks install` merges Subak's hook entries into a Claude
//!   Code settings file (by default the user's `~/.claude/settings.json`): a backup first, every
//!   other key, hook and byte of formatting kept (an order-preserving JSON reader/writer below),
//!   the file written in place so its permissions stay. `uninstall` removes exactly Subak's
//!   entries. Nothing happens without the command.
//! - **Reporter.** Each hook runs `subak claude report`: it reads the hook's JSON from stdin and
//!   forwards only `{pane_id, session_id, event, reason}` to the Host of its pane
//!   (`SUBAK_PANE_ID`, `SUBAK_ENDPOINT`) as a `hook` client — never prompts, tool input or
//!   output, transcript paths or messages. It prints nothing (Claude adds some hooks' stdout to
//!   the conversation) and always exits 0, so it never disturbs Claude.
//! - The Host authenticates a report by the **reporter's process being in the pane's job**
//!   (the OS-reported client pid), keeps per-pane sessions in memory and answers `claude.status`.

use serde_json::{Value, json};

/// The Claude Code hook events Subak listens to, and the matcher each entry gets.
pub const EVENTS: [(&str, Option<&str>); 6] = [
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PostToolUse", Some("*")),
    ("Notification", None),
    ("Stop", None),
    ("SessionEnd", None),
];

/// The most of a hook's stdin the reporter reads (inputs with tool output can be large; the
/// rest is read and dropped so Claude's write never blocks).
pub const MAX_INPUT: usize = 4 << 20;

/// What a report carries: only identifiers, the event and a coarse reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub session_id: String,
    pub event: String,
    pub reason: Option<&'static str>,
}

fn token(v: &Value, max: usize) -> Option<String> {
    let s = v.as_str()?;
    (!s.is_empty()
        && s.len() <= max
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| s.to_string())
}

/// Reads the fields Subak needs from a hook's JSON input. Anything else in it — prompts, tool
/// input and output, messages, paths — is never looked at again.
pub fn parse_input(input: &[u8]) -> Result<Report, &'static str> {
    let v: Value = serde_json::from_slice(input).map_err(|_| "the hook input is not JSON")?;
    let event = token(&v["hook_event_name"], 64).ok_or("no hook_event_name")?;
    let session_id = token(&v["session_id"], 128).ok_or("no session_id")?;
    let reason = match event.as_str() {
        "SessionStart" => match v["source"].as_str() {
            Some("startup") => Some("startup"),
            Some("resume") => Some("resume"),
            Some("clear") => Some("clear"),
            Some("compact") => Some("compact"),
            _ => Some("other"),
        },
        "Notification" => {
            let kind = v["notification_type"].as_str().unwrap_or("");
            let msg = v["message"].as_str().unwrap_or("").to_ascii_lowercase();
            Some(
                if kind == "permission_prompt" || msg.contains("permission") {
                    "permission"
                } else if kind == "idle_prompt" || msg.contains("waiting for your input") {
                    "idle"
                } else {
                    "attention"
                },
            )
        }
        "SessionEnd" => Some(match v["reason"].as_str() {
            Some("clear") => "clear",
            Some("logout") => "logout",
            Some("prompt_input_exit") => "exit",
            _ => "other",
        }),
        _ => None,
    };
    Ok(Report {
        session_id,
        event,
        reason,
    })
}

/// Step 27: the state a report moves a session to (ADR 0010 §4); `None`: the session ends.
/// Events Subak does not know (a newer Claude Code) keep the state: honest, never a guess.
pub fn next_state(event: &str, reason: Option<&str>, now: &str) -> Option<String> {
    Some(
        match (event, reason) {
            ("SessionEnd", _) => return None,
            ("SessionStart", _) => "idle",
            ("UserPromptSubmit" | "PostToolUse" | "PreToolUse", _) => "in-progress",
            ("Notification", Some("idle")) | ("Stop", _) => "done",
            ("Notification", _) => "need-input",
            ("StopFailure", _) => "error",
            _ => now,
        }
        .to_string(),
    )
}

// ---------------------------------------------------------------------------------------------
// Order-preserving JSON: settings files are rewritten with their keys, strings and numbers
// exactly as they were (only Subak's entries change).

#[derive(Debug, Clone, PartialEq)]
pub enum J {
    /// A number, `true`, `false` or `null`, as written.
    Lit(String),
    /// A string as written, quotes and escapes included.
    Str(String),
    Arr(Vec<J>),
    /// Keys as written (quoted).
    Obj(Vec<(String, J)>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.i))
    }

    fn string(&mut self) -> Result<String, String> {
        let start = self.i;
        self.i += 1;
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    let raw = std::str::from_utf8(&self.s[start..self.i])
                        .map_err(|_| "not UTF-8".to_string())?;
                    // Checked by the real parser: escapes and control characters.
                    serde_json::from_str::<String>(raw).map_err(|e| e.to_string())?;
                    return Ok(raw.to_string());
                }
                _ => self.i += 1,
            }
        }
        self.err("an unterminated string")
    }

    fn value(&mut self, depth: usize) -> Result<J, String> {
        if depth > 64 {
            return self.err("nesting too deep");
        }
        self.ws();
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(J::Obj(out));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return self.err("a key was expected");
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return self.err("`:` was expected");
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    out.push((k, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(J::Obj(out));
                        }
                        _ => return self.err("`,` or `}` was expected"),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(J::Arr(out));
                }
                loop {
                    out.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(J::Arr(out));
                        }
                        _ => return self.err("`,` or `]` was expected"),
                    }
                }
            }
            Some(b'"') => Ok(J::Str(self.string()?)),
            Some(_) => {
                let start = self.i;
                while self.i < self.s.len()
                    && !matches!(
                        self.s[self.i],
                        b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n'
                    )
                {
                    self.i += 1;
                }
                let raw = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                serde_json::from_str::<Value>(raw)
                    .ok()
                    .filter(|v| !v.is_string() && !v.is_array() && !v.is_object())
                    .map(|_| J::Lit(raw.to_string()))
                    .ok_or_else(|| format!("an invalid value at byte {start}"))
            }
            None => self.err("unexpected end"),
        }
    }
}

pub fn parse(text: &str) -> Result<J, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = P {
        s: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return p.err("text after the JSON value");
    }
    Ok(v)
}

/// Pretty JSON with two-space indents (Claude Code's own style), `nl` as the line end.
pub fn write(v: &J, nl: &str) -> String {
    fn go(v: &J, ind: usize, nl: &str, out: &mut String) {
        let pad = |n: usize| "  ".repeat(n);
        match v {
            J::Lit(s) | J::Str(s) => out.push_str(s),
            J::Arr(a) if a.is_empty() => out.push_str("[]"),
            J::Obj(o) if o.is_empty() => out.push_str("{}"),
            J::Arr(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    out.push_str(if i == 0 { "" } else { "," });
                    out.push_str(nl);
                    out.push_str(&pad(ind + 1));
                    go(x, ind + 1, nl, out);
                }
                out.push_str(nl);
                out.push_str(&pad(ind));
                out.push(']');
            }
            J::Obj(o) => {
                out.push('{');
                for (i, (k, x)) in o.iter().enumerate() {
                    out.push_str(if i == 0 { "" } else { "," });
                    out.push_str(nl);
                    out.push_str(&pad(ind + 1));
                    out.push_str(k);
                    out.push_str(": ");
                    go(x, ind + 1, nl, out);
                }
                out.push_str(nl);
                out.push_str(&pad(ind));
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    go(v, 0, nl, &mut out);
    out.push_str(nl);
    out
}

fn quoted(key: &str) -> String {
    format!("\"{key}\"")
}

fn get_mut<'a>(o: &'a mut [(String, J)], key: &str) -> Option<&'a mut J> {
    let q = quoted(key);
    o.iter_mut().find(|(k, _)| *k == q).map(|(_, v)| v)
}

/// Whether a hook command is Subak's reporter.
pub fn is_ours(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    c.contains("subak") && c.contains("claude report")
}

fn command_of(entry: &J) -> Option<String> {
    let J::Obj(o) = entry else { return None };
    o.iter()
        .find(|(k, _)| k == "\"command\"")
        .and_then(|(_, v)| match v {
            J::Str(s) => serde_json::from_str::<String>(s).ok(),
            _ => None,
        })
}

/// Per event: whether Subak's hook is there, and how many other hook commands there are.
pub fn hooks_status(settings: &J) -> Value {
    let mut events = serde_json::Map::new();
    let mut others = 0;
    let hooks = match settings {
        J::Obj(o) => o.iter().find(|(k, _)| k == "\"hooks\"").map(|(_, v)| v),
        _ => None,
    };
    if let Some(J::Obj(h)) = hooks {
        for (k, groups) in h {
            let name = serde_json::from_str::<String>(k).unwrap_or_default();
            let mut ours = false;
            if let J::Arr(gs) = groups {
                for g in gs {
                    let J::Obj(g) = g else { continue };
                    if let Some((_, J::Arr(list))) = g.iter().find(|(k, _)| k == "\"hooks\"") {
                        for e in list {
                            match command_of(e) {
                                Some(c) if is_ours(&c) => ours = true,
                                _ => others += 1,
                            }
                        }
                    }
                }
            }
            if EVENTS.iter().any(|(e, _)| *e == name) || ours {
                events.insert(name, json!(ours));
            }
        }
    }
    for (e, _) in EVENTS {
        events.entry(e.to_string()).or_insert(json!(false));
    }
    let installed = EVENTS.iter().all(|(e, _)| events[*e] == true);
    json!({"installed": installed, "events": events, "other_hooks": others})
}

/// Adds Subak's hook to every event that lacks it. Returns the events it was added to.
pub fn install(settings: &mut J, command: &str) -> Result<Vec<String>, String> {
    let J::Obj(root) = settings else {
        return Err("the settings file is not a JSON object".into());
    };
    if get_mut(root, "hooks").is_none() {
        root.push((quoted("hooks"), J::Obj(Vec::new())));
    }
    let Some(J::Obj(hooks)) = get_mut(root, "hooks") else {
        return Err("`hooks` in the settings file is not an object".into());
    };
    let mut added = Vec::new();
    for (event, matcher) in EVENTS {
        if get_mut(hooks, event).is_none() {
            hooks.push((quoted(event), J::Arr(Vec::new())));
        }
        let Some(J::Arr(groups)) = get_mut(hooks, event) else {
            return Err(format!("`hooks.{event}` is not an array"));
        };
        let present = groups.iter().any(|g| match g {
            J::Obj(g) => g.iter().any(|(k, v)| {
                k == "\"hooks\""
                    && matches!(v, J::Arr(l) if l.iter().any(|e| command_of(e).is_some_and(|c| is_ours(&c))))
            }),
            _ => false,
        });
        if present {
            continue;
        }
        let mut group = Vec::new();
        if let Some(m) = matcher {
            group.push((quoted("matcher"), J::Str(quoted(m))));
        }
        group.push((
            quoted("hooks"),
            J::Arr(vec![J::Obj(vec![
                (quoted("type"), J::Str(quoted("command"))),
                (
                    quoted("command"),
                    J::Str(serde_json::to_string(command).expect("a string")),
                ),
                (quoted("timeout"), J::Lit("10".into())),
            ])]),
        ));
        groups.push(J::Obj(group));
        added.push(event.to_string());
    }
    Ok(added)
}

/// Removes exactly Subak's hook entries (and the groups and event lists they leave empty).
/// Returns the events they were removed from.
pub fn uninstall(settings: &mut J) -> Vec<String> {
    let mut removed = Vec::new();
    let J::Obj(root) = settings else {
        return removed;
    };
    let Some(J::Obj(hooks)) = get_mut(root, "hooks") else {
        return removed;
    };
    for (k, groups) in hooks.iter_mut() {
        let J::Arr(gs) = groups else { continue };
        let before = gs.len();
        let mut hit = false;
        for g in gs.iter_mut() {
            let J::Obj(g) = g else { continue };
            if let Some(J::Arr(list)) = get_mut(g, "hooks") {
                let n = list.len();
                list.retain(|e| !command_of(e).is_some_and(|c| is_ours(&c)));
                hit |= list.len() != n;
            }
        }
        // A group Subak emptied goes; groups that were empty before stay.
        if hit {
            gs.retain(|g| !matches!(g, J::Obj(g) if g.iter().any(|(k, v)| k == "\"hooks\"" && matches!(v, J::Arr(l) if l.is_empty()))));
            removed.push(serde_json::from_str::<String>(k).unwrap_or_default());
        }
        let _ = before;
    }
    hooks.retain(|(k, v)| {
        !(removed.contains(&serde_json::from_str::<String>(k).unwrap_or_default())
            && matches!(v, J::Arr(a) if a.is_empty()))
    });
    if hooks.is_empty() && !removed.is_empty() {
        root.retain(|(k, _)| k != "\"hooks\"");
    }
    removed
}

// ---------------------------------------------------------------------------------------------
// The CLI.

use crate::cli::{Args, Cmd};
use crate::output::{CliError, Code};
use std::path::PathBuf;

/// The user's Claude Code settings file: `%USERPROFILE%\\.claude\\settings.json` (or `$HOME`).
fn default_settings() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".claude").join("settings.json"))
}

/// The hook command: this `subak` by its full path (forward slashes, quoted, so Claude Code's
/// shell on Windows — Git Bash or cmd — runs it as it is).
fn reporter_command() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "subak".into());
    format!("\"{}\" claude report", exe.replace('\\', "/"))
}

/// `subak claude hooks status|install|uninstall [--settings <file>] [--dry-run]`.
pub fn hooks(cmd: Cmd, a: &Args) -> Result<(Value, String), CliError> {
    let path = a
        .settings
        .as_ref()
        .map(PathBuf::from)
        .or_else(default_settings)
        .ok_or_else(|| CliError::new(Code::Io, "cannot find the user profile directory"))?;
    let shown = path.display().to_string();
    let original = match std::fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(CliError::new(
                Code::Io,
                format!("cannot read `{shown}`: {e}"),
            ));
        }
    };
    let mut j = match &original {
        Some(t) if !t.trim().is_empty() => parse(t).map_err(|e| {
            CliError::new(
                Code::ConfigInvalid,
                format!("`{shown}` is not valid JSON ({e}); nothing was changed"),
            )
            .hint("fix the file in an editor, then run the command again")
        })?,
        _ => J::Obj(Vec::new()),
    };
    if cmd == Cmd::ClaudeHooksStatus {
        let mut st = hooks_status(&j);
        st["settings"] = json!(shown);
        st["exists"] = json!(original.is_some());
        st["command"] = json!(reporter_command());
        let human = format!(
            "{}: Subak's Claude Code hooks are {} ({} other hook command(s) left alone)\n",
            shown,
            if st["installed"] == true {
                "installed"
            } else {
                "not installed"
            },
            st["other_hooks"]
        );
        return Ok((st, human));
    }
    let command = reporter_command();
    let changed = if cmd == Cmd::ClaudeHooksInstall {
        install(&mut j, &command).map_err(|m| {
            CliError::new(
                Code::ConfigInvalid,
                format!("`{shown}`: {m}; nothing was changed"),
            )
        })?
    } else {
        uninstall(&mut j)
    };
    let verb = if cmd == Cmd::ClaudeHooksInstall {
        "added"
    } else {
        "removed"
    };
    let mut data = json!({
        "settings": shown, "events": changed, "command": command,
        "dry_run": a.dry_run, "backup": null,
    });
    if a.dry_run || changed.is_empty() {
        let human = if changed.is_empty() {
            format!("{shown}: nothing to change\n")
        } else {
            format!(
                "{shown}: would have {verb} Subak's hook for {}\n",
                changed.join(", ")
            )
        };
        return Ok((data, human));
    }
    let nl = if original.as_deref().is_some_and(|t| t.contains("\r\n")) {
        "\r\n"
    } else {
        "\n"
    };
    let text = write(&j, nl);
    // A backup first; then the file is written in place so its permissions stay.
    if let Some(orig) = &original {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // Never over an earlier backup: `-1`, `-2`, … when the name is taken.
        let mut n = 0;
        let backup = loop {
            let name = if n == 0 {
                format!("{shown}.subak-backup-{stamp}")
            } else {
                format!("{shown}.subak-backup-{stamp}-{n}")
            };
            let b = PathBuf::from(name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&b)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    f.write_all(orig.as_bytes()).map_err(|e| {
                        CliError::new(
                            Code::Io,
                            format!(
                                "cannot write the backup `{}`: {e}; nothing was changed",
                                b.display()
                            ),
                        )
                    })?;
                    break b;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
                Err(e) => {
                    return Err(CliError::new(
                        Code::Io,
                        format!(
                            "cannot write the backup `{}`: {e}; nothing was changed",
                            b.display()
                        ),
                    ));
                }
            }
        };
        data["backup"] = json!(backup.display().to_string());
    } else if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            CliError::new(Code::Io, format!("cannot create `{}`: {e}", dir.display()))
        })?;
    }
    std::fs::write(&path, text.as_bytes())
        .map_err(|e| CliError::new(Code::Io, format!("cannot write `{shown}`: {e}")))?;
    let human = format!(
        "{shown}: {verb} Subak's hook for {}{}\n",
        changed.join(", "),
        data["backup"]
            .as_str()
            .map_or(String::new(), |b| format!(" (backup: {b})"))
    );
    Ok((data, human))
}

/// `subak claude report`: reads a hook's JSON from stdin and tells the pane's Host. Never
/// fails: the answer says whether it was delivered and why not.
pub fn report(a: &Args) -> Value {
    use std::io::Read;
    let mut input = Vec::new();
    let mut stdin = std::io::stdin().lock();
    let _ = (&mut stdin).take(MAX_INPUT as u64).read_to_end(&mut input);
    // The rest is read and dropped, so the writer never blocks on a full pipe.
    let _ = std::io::copy(&mut stdin, &mut std::io::sink());
    let not = |why: &str| json!({"delivered": false, "why": why});
    let r = match parse_input(&input) {
        Ok(r) => r,
        Err(why) => return not(why),
    };
    let pane = match std::env::var("SUBAK_PANE_ID") {
        Ok(p) if p.starts_with("p-") && p.len() <= 64 => p,
        _ => return not("not in a Subak pane (no SUBAK_PANE_ID)"),
    };
    if a.dry_run {
        // Exactly what would be sent, and nothing is.
        return json!({"delivered": false, "dry_run": true, "pane_id": pane,
            "session_id": r.session_id, "event": r.event, "why": "a dry run sends nothing"});
    }
    #[cfg(windows)]
    {
        let endpoint = a
            .endpoint
            .clone()
            .or_else(|| {
                std::env::var("SUBAK_ENDPOINT")
                    .ok()
                    .filter(|e| !e.is_empty())
            })
            .or_else(|| crate::win::pipe::default_endpoint().ok());
        let Some(endpoint) = endpoint else {
            return not("cannot determine the current user");
        };
        let who = crate::client::Identity {
            kind: crate::proto::ClientKind::Hook,
            name: "subak-claude-hook",
        };
        let mut s = match crate::client::Session::try_open_as(
            &endpoint,
            std::time::Duration::from_millis(500),
            who,
        ) {
            Ok(s) => s,
            Err(_) => return not("the Subak Host is not running"),
        };
        let params = json!({"pane_id": pane, "session_id": r.session_id, "event": r.event,
            "reason": r.reason});
        match s.request_within("claude.report", &params, std::time::Duration::from_secs(2)) {
            Ok(res) => json!({"delivered": true, "pane_id": pane, "session_id": r.session_id,
                "event": r.event, "state": res["state"]}),
            Err(e) => json!({"delivered": false, "why": e.message}),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (a, r, pane);
        not("Subak's Host runs on Windows only for now")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = r#"{
  "model": "opus",
  "permissions": {"allow": ["Bash(ls:*)"], "deny": []},
  "hooks": {
    "Stop": [
      {"hooks": [{"type": "command", "command": "notify-send \"done \\u00e9\""}]}
    ],
    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "audit.sh"}]}]
  },
  "zeta": 1.50e3,
  "env": {"A": "한글"}
}"#;

    #[test]
    fn install_and_uninstall_keep_everything_else_exactly() {
        let mut j = parse(USER).unwrap();
        let st = hooks_status(&j);
        assert_eq!(st["installed"], false);
        assert_eq!(st["other_hooks"], 2);
        let cmd = r#""C:/Tools/subak.exe" claude report"#;
        let added = install(&mut j, cmd).unwrap();
        assert_eq!(added.len(), 6);
        assert!(install(&mut j, cmd).unwrap().is_empty(), "idempotent");
        let text = write(&j, "\n");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["zeta"], 1500.0);
        assert_eq!(v["env"]["A"], "한글");
        assert!(text.contains("1.50e3"), "numbers as written");
        assert!(
            text.contains(r#""notify-send \"done \\u00e9\"""#),
            "strings as written"
        );
        assert_eq!(
            v["hooks"]["Stop"].as_array().unwrap().len(),
            2,
            "the user's Stop hook stays first"
        );
        assert_eq!(v["hooks"]["Stop"][1]["hooks"][0]["command"], cmd);
        assert_eq!(v["hooks"]["PostToolUse"][0]["matcher"], "*");
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "audit.sh"
        );
        // Key order is kept: model before permissions before hooks before zeta.
        let pos = |k: &str| text.find(&format!("\"{k}\"")).unwrap();
        assert!(pos("model") < pos("permissions") && pos("hooks") < pos("zeta"));
        let st = hooks_status(&j);
        assert_eq!(
            (st["installed"].clone(), st["other_hooks"].clone()),
            (json!(true), json!(2))
        );

        let removed = uninstall(&mut j);
        assert_eq!(removed.len(), 6);
        assert_eq!(
            serde_json::from_str::<Value>(&write(&j, "\n")).unwrap(),
            serde_json::from_str::<Value>(USER).unwrap(),
            "uninstall restores the user's settings"
        );
        // A file without hooks gets them and loses them again entirely.
        let mut e = parse("{}").unwrap();
        install(&mut e, cmd).unwrap();
        uninstall(&mut e);
        assert_eq!(write(&e, "\n"), "{}\n");
    }

    #[test]
    fn invalid_settings_are_refused_not_rewritten() {
        for bad in [
            "",
            "{",
            "{\"a\": }",
            "[1,]",
            "{\"a\": 1} x",
            "{\"a\": \"\u{1}\"}",
            "tru",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let mut arr = parse("[]").unwrap();
        assert!(install(&mut arr, "subak claude report").is_err());
        let mut odd = parse(r#"{"hooks": []}"#).unwrap();
        assert!(install(&mut odd, "subak claude report").is_err());
        assert!(parse("\u{feff}{\"a\": [true, null, -1]}").is_ok(), "a BOM");
    }

    #[test]
    fn reports_carry_only_ids_events_and_coarse_reasons() {
        let r = parse_input(
            br#"{"session_id":"abc-123","hook_event_name":"UserPromptSubmit","prompt":"my secret ghp_x","transcript_path":"C:/u/t.jsonl","cwd":"C:/w"}"#,
        )
        .unwrap();
        assert_eq!(
            r,
            Report {
                session_id: "abc-123".into(),
                event: "UserPromptSubmit".into(),
                reason: None
            }
        );
        let n = |j: &str| parse_input(j.as_bytes()).unwrap().reason;
        assert_eq!(
            n(
                r#"{"session_id":"s","hook_event_name":"Notification","notification_type":"permission_prompt","message":"x"}"#
            ),
            Some("permission")
        );
        assert_eq!(
            n(
                r#"{"session_id":"s","hook_event_name":"Notification","message":"Claude needs your permission to use Bash"}"#
            ),
            Some("permission")
        );
        assert_eq!(
            n(
                r#"{"session_id":"s","hook_event_name":"Notification","notification_type":"idle_prompt"}"#
            ),
            Some("idle")
        );
        assert_eq!(
            n(r#"{"session_id":"s","hook_event_name":"SessionStart","source":"resume"}"#),
            Some("resume")
        );
        assert_eq!(
            n(r#"{"session_id":"s","hook_event_name":"SessionEnd","reason":"prompt_input_exit"}"#),
            Some("exit")
        );
        // Malformed or hostile identifiers are refused, never passed on.
        for bad in [
            r#"{"hook_event_name":"Stop"}"#,
            r#"{"session_id":"a b","hook_event_name":"Stop"}"#,
            r#"{"session_id":"s","hook_event_name":"Stop\nX"}"#,
            "not json",
        ] {
            assert!(parse_input(bad.as_bytes()).is_err(), "{bad}");
        }
        // States (ADR 0010 §4); unknown events keep the state.
        assert_eq!(
            next_state("UserPromptSubmit", None, "idle").as_deref(),
            Some("in-progress")
        );
        assert_eq!(
            next_state("Notification", Some("permission"), "in-progress").as_deref(),
            Some("need-input")
        );
        assert_eq!(
            next_state("Notification", Some("idle"), "done").as_deref(),
            Some("done")
        );
        assert_eq!(
            next_state("Stop", None, "in-progress").as_deref(),
            Some("done")
        );
        assert_eq!(
            next_state("FutureEvent", None, "need-input").as_deref(),
            Some("need-input")
        );
        assert_eq!(next_state("SessionEnd", Some("exit"), "done"), None);
    }
}

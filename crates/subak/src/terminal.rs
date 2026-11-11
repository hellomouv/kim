//! `subak new | ls | info | status | attach | detach | rename | stop | kill | rm` — the headless
//! Terminal lifecycle (docs/SESSION_MODEL.md, ADR 0020, ADR 0027, docs/SUBAK_CLI.md) over IPC v1.
//! (`subak terminal <verb>` is their deprecated Step 10 spelling.)
//!
//! Every command is a short-lived client of the per-user Host. Only `new` starts a Host on demand
//! (with no Terminal of its own, unless `--no-start`); the read commands never start one, and
//! `ls` or `status` without a Host simply report none. What each verb does to processes is fixed:
//!
//! | verb | processes | stored Terminal |
//! | --- | --- | --- |
//! | `attach`, detach (`Ctrl+B d`, `subak detach`), closing this window | untouched | untouched |
//! | `stop` | graceful (`CTRL_CLOSE_EVENT`, 5 s), then forced | kept as `stopped` |
//! | `kill` | forced at once | removed |
//! | `rm` | refused while it runs; `--force` = `kill` | removed |
//!
//! `stop`, `kill` and `rm` are destructive (CLI_CONTRACT §4): they ask first, or need `--yes`
//! when they cannot ask. Only after that consent does the client send `force`, which the Host
//! requires to end a Terminal that runs more than an idle shell.

use crate::cli::{Args, Cmd, EnvArg};
use crate::client::{self, Connected, Failure, Plan, Session, Why, confirm};
use crate::output::{CliError, Code};
use crate::{pty, session};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a create may take (pseudo console and process start).
const CREATE_TIMEOUT: Duration = Duration::from_secs(20);

fn host_not_running(endpoint: &str) -> CliError {
    CliError::new(
        Code::HostNotRunning,
        format!(
            "no Subak Host is serving `{}`, so there are no Terminals",
            crate::win::pipe::path(endpoint)
        ),
    )
    .hint("create one with `subak new`")
}

/// Connects without starting a Host; `None` when none runs.
fn connect(endpoint: &str) -> Result<Option<Session>, CliError> {
    match Session::try_open(endpoint, Duration::from_secs(2)) {
        Ok(s) => Ok(Some(s)),
        Err(Failure {
            why: Why::NotRunning,
            ..
        }) => Ok(None),
        Err(f) => Err(f.err),
    }
}

fn require(endpoint: &str) -> Result<Session, CliError> {
    connect(endpoint)?.ok_or_else(|| host_not_running(endpoint))
}

fn target(a: &Args) -> &str {
    a.positional.first().map_or("", String::as_str)
}

fn size_json(a: &Args) -> Value {
    a.size
        .map_or(Value::Null, |(c, r)| json!({"cols": c, "rows": r}))
}

/// The `env` parameter of `terminal.create` from `--env` / `--unset-env`. `--env NAME` copies
/// this process's value, so a token never has to appear on a command line. No message ever
/// contains a value.
fn env_param(a: &Args) -> Result<Value, CliError> {
    if a.env.is_empty() {
        return Ok(Value::Null);
    }
    let mut map = serde_json::Map::new();
    let mut seen: Vec<&str> = Vec::new();
    for e in &a.env {
        let name = e.name();
        pty::check_env_name(name).map_err(CliError::usage)?;
        if seen.iter().any(|s| pty::same_var(s, name)) {
            return Err(CliError::usage(format!(
                "the variable `{name}` is given twice (names ignore case)"
            )));
        }
        seen.push(name);
        let value = match e {
            EnvArg::Set(_, v) => Value::from(v.as_str()),
            EnvArg::Unset(_) => Value::Null,
            EnvArg::Copy(_) => match std::env::var(name) {
                Ok(v) => Value::from(v),
                Err(std::env::VarError::NotPresent) => {
                    return Err(CliError::usage(format!(
                        "`--env {name}` copies `{name}` from this environment, but it is not set here"
                    ))
                    .hint(format!("give a value with --env {name}=VALUE")));
                }
                Err(std::env::VarError::NotUnicode(_)) => {
                    return Err(CliError::usage(format!(
                        "the value of `{name}` in this environment is not valid Unicode"
                    )));
                }
            },
        };
        map.insert(name.to_string(), value);
    }
    Ok(Value::Object(map))
}

/// `new` with a shell profile of `config.toml` applied (Step 20, ADR 0051 §3): the profile's
/// program and arguments unless `-- <program>` or `--command-line` is given, its working
/// directory unless `--cwd` is, and its environment overrides first, so an `--env` or
/// `--unset-env` of the same variable wins. The Host receives explicit values only; it never
/// reads profiles.
fn with_profile(a: &Args, profile: Option<&crate::config::Profile>) -> Args {
    let mut eff = a.clone();
    let Some(p) = profile else {
        return eff;
    };
    if a.command.is_empty() && a.command_line.is_none() {
        eff.command = std::iter::once(p.program.clone())
            .chain(p.args.iter().cloned())
            .collect();
    }
    if a.cwd.is_none() {
        eff.cwd.clone_from(&p.cwd);
    }
    let given = |n: &str| a.env.iter().any(|e| pty::same_var(e.name(), n));
    let mut env: Vec<EnvArg> = p
        .env
        .iter()
        .filter(|(n, _)| !given(n))
        .map(|(n, v)| EnvArg::Set(n.clone(), v.clone()))
        .collect();
    env.extend(
        p.unset_env
            .iter()
            .filter(|n| !given(n))
            .map(|n| EnvArg::Unset(n.clone())),
    );
    env.extend(a.env.iter().cloned());
    eff.env = env;
    eff
}

/// A dry run without a Host: the same checks the Host would make (name, working directory,
/// the program on the pane's PATH), on this machine, as this user, with nothing started.
fn plan_without_host(a: &Args, params: &Value) -> Result<Value, CliError> {
    let name = match a.positional.first() {
        Some(n) => {
            session::parse_name(n)
                .map_err(|e| CliError::new(Code::InvalidName, e.message))?
                .display
        }
        None => "1".into(),
    };
    let command = match (a.command.split_first(), &a.command_line) {
        (Some((program, args)), _) => session::Command {
            program: program.clone(),
            args: args.to_vec(),
        },
        (None, Some(line)) => session::split_command_line(line),
        (None, None) => session::split_command_line(&crate::host::default_shell()),
    };
    if a.command_line.is_some() && command.program.is_empty() {
        return Err(CliError::usage("--command-line must start with a program"));
    }
    let env = pty::EnvOverrides::from_json(&params["env"]).map_err(CliError::usage)?;
    let cwd = match &a.cwd {
        Some(c) if !Path::new(c).is_absolute() => {
            return Err(CliError::usage("--cwd must be an absolute path"));
        }
        Some(c) => PathBuf::from(c),
        None => crate::host::profile_dir(crate::win::EnvBlock::for_current_user().ok().as_ref())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default(),
    };
    let executable = pty::plan(&pty::Spec {
        program: &command.program,
        args: &command.args,
        raw_line: a.command_line.as_deref(),
        cwd: &cwd,
        env: &env,
        host_vars: &[],
        size: a.size.unwrap_or(crate::host::DEFAULT_SIZE),
    })
    .map_err(|e| {
        let mut err = CliError::new(Code::SpawnFailed, e.message)
            .detail(format!("reason: {}", e.reason.as_str()));
        if let Some(h) = e.hint {
            err = err.hint(h);
        }
        err
    })?;
    Ok(
        json!({"dry_run": true, "would_start_host": !a.no_start, "would_create": {
            "name": name,
            "command": {"program": crate::redact::program(&command.program), "args": crate::redact::args(&command.args)},
            "command_line": a.command_line.as_deref().map(crate::redact::command_line),
            "executable": executable.display().to_string(),
            "cwd": cwd.display().to_string(),
            "env": session::env_json(&env.names()),
            "size": params["size"].as_object().map_or_else(
                || json!({"cols": crate::host::DEFAULT_SIZE.0, "rows": crate::host::DEFAULT_SIZE.1}),
                |s| Value::Object(s.clone())),
            "remain_on_exit": a.remain_on_exit, "keep_on_failure": a.keep_on_failure,
            "history_limit": a.history_limit.unwrap_or(crate::screen::DEFAULT_HISTORY_LINES as u64),
        }}),
    )
}

/// `terminal new [<name>] [--cwd <dir>] [--env NAME[=VALUE]]… [--unset-env NAME]…
/// [--size CxR] [--remain-on-exit] [--keep-on-failure] [-- <program> [args…]]`.
pub fn new(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    // Step 20: a shell profile (`--profile`, or the default one) becomes explicit parameters.
    let (profile, warnings) = crate::config_cmd::profile_for_new(a)?;
    for w in warnings {
        crate::warn_later(w);
    }
    let profile_name = profile.as_ref().map(|p| p.name.clone());
    let eff = with_profile(a, profile.as_ref());
    let a = &eff;
    // `profile` is added only when a profile was used, so the result of a plain `new` stays the
    // `terminal.create` result it always was (additive, CLI_CONTRACT §8).
    let with_name = |mut v: Value| {
        if let (Some(o), Some(p)) = (v.as_object_mut(), &profile_name) {
            o.insert("profile".into(), json!(p));
        }
        v
    };
    let command = match a.command.split_first() {
        Some((program, args)) => json!({"program": program, "args": args}),
        None => Value::Null,
    };
    let params = json!({
        "name": a.positional.first(),
        "command": command,
        "command_line": a.command_line,
        "cwd": a.cwd,
        "env": env_param(a)?,
        "size": size_json(a),
        "remain_on_exit": a.remain_on_exit,
        "keep_on_failure": a.keep_on_failure,
        "history_limit": a.history_limit,
    });
    if a.dry_run {
        let mut p = params;
        p["dry_run"] = json!(true);
        return match connect(endpoint)? {
            Some(mut s) => {
                let mut plan = s.request("terminal.create", &p)?;
                plan["would_start_host"] = json!(false);
                Ok(with_name(plan))
            }
            // The error the real run would hit (CLI_CONTRACT §4.2).
            None if a.no_start => Err(no_start(endpoint)),
            // Without a Host there is no name to collide with; everything else is checked.
            None => plan_without_host(a, &p).map(with_name),
        };
    }
    let (mut session, started) = if a.no_start {
        (connect(endpoint)?.ok_or_else(|| no_start(endpoint))?, None)
    } else {
        // Start on demand: a Host without a Terminal of its own (ADR 0019 §5), under the
        // per-endpoint start lock, so concurrent `new` calls share one Host.
        let plan = Plan {
            shell: None,
            idle_exit_s: crate::host::DEFAULT_IDLE_EXIT_S,
            state_dir: None,
        };
        let Connected {
            session, started, ..
        } = client::connect_or_start(endpoint, &plan)?;
        (session, started)
    };
    let mut created = session.request_within("terminal.create", &params, CREATE_TIMEOUT)?;
    created["host"] = json!({"pid": session.host_pid, "started": started.is_some()});
    let created = with_name(created);
    if !a.attach {
        return Ok(created);
    }
    // `new --attach`: the creation is reported as a notice, then this terminal attaches.
    drop(session);
    if !a.quiet {
        eprintln!(
            "[subak: created Terminal `{}` ({}){}]",
            created["name"].as_str().unwrap_or("?"),
            created["terminal_id"].as_str().unwrap_or("?"),
            if started.is_some() {
                format!("; started Host pid {}", created["host"]["pid"])
            } else {
                String::new()
            }
        );
    }
    let id = created["terminal_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let attached = client::attach(a, endpoint, Some(&id))?;
    Ok(json!({"created": created, "attach": attached}))
}

fn no_start(endpoint: &str) -> CliError {
    CliError::new(
        Code::HostNotRunning,
        format!(
            "no Subak Host is serving `{}`, and --no-start forbids starting one",
            crate::win::pipe::path(endpoint)
        ),
    )
    .hint("run without --no-start to start one on demand, or start it with `subak host start --no-shell`")
}

/// `ls`: every Terminal of the Host, in creation order. No Host: an empty list.
pub fn list(endpoint: &str) -> Result<Value, CliError> {
    let Some(mut s) = connect(endpoint)? else {
        return Ok(json!({"terminals": [], "host": null}));
    };
    let mut r = s.request("terminal.list", &json!({"detail": "summary"}))?;
    r["host"] = json!({"pid": s.host_pid});
    Ok(r)
}

/// `status`: the Host (or that none runs), every Terminal, and every client, in one answer.
/// Starts nothing; exits 0 also without a Host.
pub fn status(endpoint: &str) -> Result<Value, CliError> {
    let Some(mut s) = connect(endpoint)? else {
        return Ok(json!({
            "running": false, "endpoint": crate::win::pipe::path(endpoint),
            "host": null, "terminals": [], "clients": null,
        }));
    };
    let st = s.request("host.status", &json!({}))?;
    let list = s.request("terminal.list", &json!({"detail": "summary"}))?;
    Ok(json!({
        "running": true, "endpoint": st["endpoint"], "host": st["host"],
        "terminals": list["terminals"], "clients": st["clients"],
        "scrollback": st["scrollback"],
    }))
}

/// `info <target>`: one Terminal; exit 3 (`target_not_found`) when there is none (tmux
/// `has-session`).
pub fn info(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    require(endpoint)?.request("terminal.info", &json!({"target": target(a)}))
}

/// `detach [<target>] [--client <id>]`: ends attachments of clients — every client of the
/// Terminal, one client, or that client's attachment to the Terminal — and nothing else. Inside a
/// pane, no target means the pane's own Terminal (`SUBAK_TERMINAL_ID`, ADR 0010 §1). Detaching
/// never stops a process, a Terminal or the Host.
pub fn detach(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let target = match (a.positional.first(), a.client) {
        (Some(t), _) => Some(t.clone()),
        (None, Some(_)) => None,
        (None, None) => match std::env::var("SUBAK_TERMINAL_ID") {
            Ok(id) if session::is_id(&id, 't') => Some(id),
            _ => {
                return Err(CliError::usage(
                    "`detach` needs a Terminal or --client <id> outside a Subak pane",
                )
                .hint("e.g. `subak detach work` detaches every client of `work`; `subak status` lists client ids"));
            }
        },
    };
    let mut s = require(endpoint)?;
    let mut r = s.request(
        "client.detach",
        &json!({"target": target, "client_id": a.client, "dry_run": a.dry_run}),
    )?;
    r["host"] = json!({"pid": s.host_pid});
    Ok(r)
}

/// The pane `capture`, `history` and `clear-history` mean: the named target; inside a pane
/// without one, that pane (`SUBAK_PANE_ID`, ADR 0010 §1, like tmux's current pane); else none
/// (the Host's only Terminal).
fn screen_target(a: &Args) -> Value {
    match a.positional.first() {
        Some(t) => json!(t),
        None => match std::env::var("SUBAK_PANE_ID") {
            Ok(id) if session::is_id(&id, 'p') => json!(id),
            _ => Value::Null,
        },
    }
}

fn line_json(l: Option<crate::cli::Line>) -> Value {
    match l {
        None => Value::Null,
        Some(crate::cli::Line::Edge) => json!("-"),
        Some(crate::cli::Line::Number(n)) => json!(n),
    }
}

/// Most `pane.capture` requests one `capture` makes to collect a long history.
const CAPTURE_PAGES: usize = 64;

/// `capture [<target>]` (tmux `capture-pane -p`): the screen, or a range of history and screen
/// lines, as text (or with `--vt` escape sequences); secret-looking values are hidden unless
/// `--no-redact`. A long range is collected in several requests (each answer is one message).
pub fn capture(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let mut s = require(endpoint)?;
    let start = if a.history {
        json!("-")
    } else {
        line_json(a.start)
    };
    let mut params = json!({
        "target": screen_target(a),
        "start": start,
        "end": line_json(a.end),
        "format": if a.vt { "vt" } else { "text" },
        "join": a.join,
        "redact": !a.no_redact,
    });
    let mut r = s.request("pane.capture", &params)?;
    // Later pages name the pane by id, so they read the same pane.
    params["target"] = r["pane_id"].clone();
    let mut lines: Vec<Value> = r["lines"].as_array().cloned().unwrap_or_default();
    let mut redacted = r["redacted"].as_u64().unwrap_or(0);
    let mut requests = 1;
    let end = r["end"].clone();
    while r["truncated"] == true && requests < CAPTURE_PAGES {
        let Some(first) = r["start"].as_i64() else {
            break;
        };
        params["end"] = json!(first - 1);
        r = s.request("pane.capture", &params)?;
        requests += 1;
        redacted += r["redacted"].as_u64().unwrap_or(0);
        let mut page = r["lines"].as_array().cloned().unwrap_or_default();
        page.extend(lines);
        lines = page;
    }
    let name =
        s.request("terminal.info", &json!({"target": r["terminal_id"]}))?["terminal"]["name"]
            .clone();
    r["lines"] = Value::Array(lines);
    r["end"] = end;
    r["redacted"] = json!(redacted);
    r["name"] = name;
    r["requests"] = json!(requests);
    Ok(r)
}

/// `history [<target>]`: what the Host keeps of a Terminal's output — counters, limits and the
/// screen, never content (docs/SCROLLBACK.md).
pub fn history(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let mut s = require(endpoint)?;
    // An empty range (0 to -1) returns the counters and the screen without any line.
    let r = s.request(
        "pane.capture",
        &json!({"target": screen_target(a), "start": 0, "end": -1}),
    )?;
    let t = s.request("terminal.info", &json!({"target": r["terminal_id"]}))?["terminal"].clone();
    Ok(json!({
        "terminal_id": r["terminal_id"], "name": t["name"], "state": t["state"],
        "pane_id": r["pane_id"], "history": r["history"], "screen": r["screen"],
    }))
}

/// `clear-history [<target>]` (tmux `clear-history`): deletes the history the Host keeps for a
/// pane; the screen stays. It deletes data, so it asks first (or needs `--yes`).
pub fn clear_history(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let mut s = require(endpoint)?;
    let plan = s.request(
        "pane.clear_history",
        &json!({"target": screen_target(a), "dry_run": true}),
    )?;
    let mut would = plan["would_clear"].clone();
    let name =
        s.request("terminal.info", &json!({"target": would["terminal_id"]}))?["terminal"]["name"]
            .clone();
    would["name"] = name.clone();
    if a.dry_run {
        return Ok(json!({"dry_run": true, "would_clear": would}));
    }
    let shown = name.as_str().unwrap_or("?");
    let lines = would["lines"].as_u64().unwrap_or(0);
    if lines > 0 {
        confirm(
            a,
            &format!(
                "Delete the {lines} line(s) of history the Host keeps for Terminal `{shown}`?"
            ),
            &format!("clearing deletes {lines} line(s) of history of Terminal `{shown}`"),
            "re-run with --yes, or with --dry-run to see what would be cleared",
        )?;
    }
    // The pane the user confirmed, by id.
    let mut r = s.request(
        "pane.clear_history",
        &json!({"target": would["pane_id"], "dry_run": false}),
    )?;
    r["name"] = name;
    Ok(r)
}

/// `rename <target> <new name>`.
/// `window new <terminal>` and `pane new <terminal|window>` (Step 21, ADR 0054): a new internal
/// window with one pane, or a new pane in a window, of a running Terminal. Never starts a Host
/// (a window needs a Terminal), never ends anything.
pub fn create_in(a: &Args, endpoint: &str, window: bool) -> Result<Value, CliError> {
    let command = match a.command.split_first() {
        Some((program, args)) => json!({"program": program, "args": args}),
        None => Value::Null,
    };
    let params = json!({
        "target": target(a),
        "name": a.name,
        "command": command,
        "command_line": a.command_line,
        "cwd": a.cwd,
        "env": env_param(a)?,
        "size": size_json(a),
        "history_limit": a.history_limit,
        "select": !a.no_select,
        "dry_run": a.dry_run,
    });
    let method = if window {
        "window.create"
    } else {
        "pane.create"
    };
    require(endpoint)?.request_within(method, &params, CREATE_TIMEOUT)
}

/// `pane split <target>` (Step 22, tmux `split-window`): a new pane with its own process next
/// to the target pane (or the active pane of a target window or Terminal), `--vertical`
/// (stacked, the default) or `--horizontal` (side by side), `--before` it, at `--cells` or
/// `--percent` of the split pane (default half).
pub fn split(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let command = match a.command.split_first() {
        Some((program, args)) => json!({"program": program, "args": args}),
        None => Value::Null,
    };
    let params = json!({
        "target": target(a),
        "direction": if a.split == Some(true) { "horizontal" } else { "vertical" },
        "before": a.before,
        "cells": a.cells,
        "percent": a.percent,
        "command": command,
        "command_line": a.command_line,
        "cwd": a.cwd,
        "env": env_param(a)?,
        "history_limit": a.history_limit,
        "select": !a.no_select,
        "dry_run": a.dry_run,
    });
    require(endpoint)?.request_within("pane.split", &params, CREATE_TIMEOUT)
}

/// `pane resize <pane> <left|right|up|down> [cells]` (tmux `resize-pane -L/-R/-U/-D`),
/// `pane swap <pane> <other|next|previous>` (`swap-pane`) and `pane zoom <pane> [on|off]`
/// (`resize-pane -Z`, a toggle by default): the layout changes, every process keeps running.
pub fn layout(a: &Args, endpoint: &str, cmd: Cmd) -> Result<Value, CliError> {
    let second = a.positional.get(1).map(String::as_str);
    let (method, mut params) = match cmd {
        Cmd::PaneResize => {
            let dir = second.unwrap_or("");
            if !matches!(dir, "left" | "right" | "up" | "down") {
                return Err(CliError::usage(format!("invalid direction `{dir}`"))
                    .hint("left, right, up or down"));
            }
            let cells = match a.positional.get(2) {
                None => 1,
                Some(n) => n
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (1..=1000).contains(n))
                    .ok_or_else(|| {
                        CliError::usage(format!("invalid number of cells `{n}`")).hint("1 to 1000")
                    })?,
            };
            ("pane.adjust", json!({"direction": dir, "cells": cells}))
        }
        Cmd::PaneSwap => match second {
            Some(d @ ("next" | "previous")) => ("pane.swap", json!({"direction": d})),
            Some(other) => ("pane.swap", json!({"with": other})),
            None => return Err(CliError::usage("`pane swap` needs the other pane")),
        },
        Cmd::WindowRotate => match second {
            None | Some("up") => ("window.rotate", json!({"direction": "up"})),
            Some("down") => ("window.rotate", json!({"direction": "down"})),
            Some(x) => {
                return Err(CliError::usage(format!("invalid direction `{x}`")).hint("up or down"));
            }
        },
        Cmd::WindowLayout => {
            let l = second.unwrap_or("");
            if crate::layout::Preset::parse(l).is_none()
                && !matches!(l, "next" | "previous" | "spread")
            {
                return Err(CliError::usage(format!("unknown layout `{l}`"))
                    .hint("even-horizontal, even-vertical, main-horizontal, main-vertical (-mirrored), tiled, next, previous or spread"));
            }
            ("window.select_layout", json!({"layout": l}))
        }
        _ => {
            let zoom = match second {
                None => Value::Null,
                Some("on") => json!(true),
                Some("off") => json!(false),
                Some(x) => {
                    return Err(CliError::usage(format!("invalid zoom `{x}`"))
                        .hint("on or off (omit it to toggle)"));
                }
            };
            ("pane.zoom", json!({"zoom": zoom}))
        }
    };
    params["target"] = json!(target(a));
    params["dry_run"] = json!(a.dry_run);
    require(endpoint)?.request(method, &params)
}

/// Step 24 (ADR 0058): `subak command '<tmux command line>'` (also `subak tmux`): the tmux
/// commands of [`crate::tmuxcmd`], run one after another through the Host; the first refusal
/// stops the rest (tmux). Without `-t` a command acts on `--target`, else on the pane this runs in
/// (`SUBAK_PANE_ID`, set in every pane). `--dry-run` parses and plans only. Commands that end
/// processes need `--yes`.
pub fn command(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    use crate::tmuxcmd::{self, Context, Plan};
    let line = if a.command.is_empty() {
        a.positional.join(" ")
    } else {
        // `subak command -- split-window -h`: the words as the shell split them.
        a.command
            .iter()
            .map(|w| {
                if w == ";" {
                    w.clone()
                } else {
                    format!("'{}'", w.replace('\'', "'\\''"))
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let cmds = tmuxcmd::parse(&line).map_err(|e| {
        let err = CliError::usage(format!("tmux command: {}", e.message))
            .detail(format!("kind: {}", e.kind));
        match e.hint {
            Some(h) => err.hint(h),
            None => err,
        }
    })?;
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let mut ctx = Context {
        yes: a.yes,
        ..Context::default()
    };
    match &a.target {
        Some(t) => {
            ctx.pane = Some(t.clone());
            let (term, rest) = t
                .split_once(':')
                .map_or((t.as_str(), None), |(x, y)| (x, Some(y)));
            if !term.starts_with("p-") && !term.starts_with("w-") {
                ctx.terminal = Some(term.to_string());
            }
            if let Some(r) = rest {
                let w = r.split('.').next().unwrap_or(r);
                ctx.window = Some(format!("{term}:{w}"));
            } else if term.starts_with("w-") {
                ctx.window = Some(term.to_string());
            }
        }
        None => {
            ctx.pane = env("SUBAK_PANE_ID");
            ctx.terminal = env("SUBAK_TERMINAL_ID");
        }
    }
    let plans: Vec<(String, Plan)> = cmds
        .iter()
        .map(|c| {
            tmuxcmd::plan(c, &ctx)
                .map(|p| (c.name().to_string(), p))
                .map_err(|e| {
                    let mut err = CliError::usage(format!("{}: {}", c.name(), e.message))
                        .detail(format!("kind: {}", e.kind));
                    if e.kind == "confirmation_required" {
                        err = CliError::new(
                            Code::ConfirmationRequired,
                            format!("{}: {}", c.name(), e.message),
                        );
                    }
                    match e.hint {
                        Some(h) => err.hint(h),
                        None => err,
                    }
                })
        })
        .collect::<Result<_, _>>()?;
    if a.dry_run {
        let would: Vec<Value> = plans
            .iter()
            .map(|(name, p)| match p {
                Plan::Request { method, params } => {
                    json!({"command": name, "method": method, "params": params})
                }
                Plan::Relative { terminal, delta } => {
                    json!({"command": name, "method": "window.select", "params": {"terminal": terminal, "delta": delta}})
                }
                Plan::Local(what) => json!({"command": name, "method": null, "local": what}),
                Plan::Format { terminal, .. } => {
                    json!({"command": name, "method": "pane.list", "params": {"target": terminal}})
                }
                Plan::Text(_) => json!({"command": name, "method": null, "local": "text"}),
            })
            .collect();
        return Ok(json!({"dry_run": true, "would_run": would}));
    }
    if let Some((name, _)) = plans.iter().find(|(_, p)| matches!(p, Plan::Local(_))) {
        return Err(CliError::usage(format!("{name} works only in the Subak window"))
            .hint("in the window: Ctrl+B [ (copy mode), Ctrl+B d (detach); from a console: `subak detach <terminal>`"));
    }
    let mut s = require(endpoint)?;
    // Window commands of the context pane need its window.
    if ctx.window.is_none()
        && let (Some(t), Some(p)) = (&ctx.terminal, &ctx.pane)
        && let Ok(top) = s.request("pane.list", &json!({"target": t}))
    {
        ctx.window = top["windows"].as_array().and_then(|ws| {
            ws.iter()
                .find(|w| {
                    w["panes"]
                        .as_array()
                        .is_some_and(|ps| ps.iter().any(|x| x["id"] == p.as_str()))
                })
                .and_then(|w| w["id"].as_str().map(String::from))
        });
        // Re-plan with the window now known.
    }
    let plans: Vec<(String, Plan)> = cmds
        .iter()
        .map(|c| {
            (
                c.name().to_string(),
                tmuxcmd::plan(c, &ctx).expect("planned above"),
            )
        })
        .collect();
    let mut results = Vec::new();
    for (name, p) in plans {
        let (method, result) = match p {
            Plan::Request { method, params } => {
                let r = if matches!(method, "pane.split" | "window.create" | "terminal.create") {
                    s.request_within(method, &params, CREATE_TIMEOUT)
                } else {
                    s.request(method, &params)
                };
                (method, r)
            }
            Plan::Relative { terminal, delta } => {
                let r = s
                    .request("window.list", &json!({"target": terminal}))
                    .and_then(|l| {
                        let ws = l["windows"].as_array().cloned().unwrap_or_default();
                        let n = ws.len().max(1) as i64;
                        let at = ws
                            .iter()
                            .position(|w| w["id"] == l["active_window"])
                            .unwrap_or(0) as i64;
                        let next = &ws[usize::try_from((at + delta).rem_euclid(n)).unwrap_or(0)];
                        s.request("window.select", &json!({"target": next["id"]}))
                    });
                ("window.select", r)
            }
            Plan::Local(_) => unreachable!("refused above"),
            // Step 25: `display-message` expands formats with the Terminal's topology.
            Plan::Format {
                terminal,
                pane,
                message,
                ..
            } => {
                let r = s
                    .request("pane.list", &json!({"target": terminal}))
                    .map(|t| {
                        let v = tmuxcmd::vars(&t, pane.as_deref());
                        json!({"message": tmuxcmd::format(&message, &v)})
                    });
                ("format", r)
            }
            Plan::Text(text) => ("text", Ok(json!({"text": text}))),
        };
        match result {
            Ok(r) => results.push(json!({"command": name, "method": method, "result": r})),
            Err(e) => {
                return Err(e.detail(format!(
                    "stopped at `{name}` after {} command(s) ran",
                    results.len()
                )));
            }
        }
    }
    Ok(json!({"results": results}))
}

/// Step 24: `subak buffer ls|show|set|delete|paste` (tmux `list-buffers`, `show-buffer`,
/// `set-buffer`, `delete-buffer`, `paste-buffer`).
pub fn buffer(a: &Args, endpoint: &str, cmd: Cmd) -> Result<Value, CliError> {
    let first = a.positional.first().cloned();
    let (method, params) = match cmd {
        Cmd::BufferLs => ("buffer.list", json!({})),
        Cmd::BufferShow => ("buffer.get", json!({"name": first})),
        Cmd::BufferDelete => (
            "buffer.delete",
            json!({"name": first, "dry_run": a.dry_run}),
        ),
        Cmd::BufferSet => (
            "buffer.set",
            json!({"name": a.name, "data": first.unwrap_or_default(), "append": a.append,
                "dry_run": a.dry_run}),
        ),
        _ => (
            "buffer.paste",
            json!({"target": first, "name": a.name, "delete": a.delete, "dry_run": a.dry_run}),
        ),
    };
    require(endpoint)?.request(method, &params)
}

/// `window ls <terminal>` and `pane ls <terminal|window>`.
pub fn topology(a: &Args, endpoint: &str, panes: bool) -> Result<Value, CliError> {
    let method = if panes { "pane.list" } else { "window.list" };
    require(endpoint)?.request(method, &json!({"target": target(a)}))
}

/// `window select <window>` and `pane select <pane>`: the active window or pane every client
/// sees. No process changes.
///
/// Step 22: `pane select <pane> <left|right|up|down|next|previous>` selects the pane next to it.
pub fn select(a: &Args, endpoint: &str, pane: bool) -> Result<Value, CliError> {
    let method = if pane { "pane.select" } else { "window.select" };
    let mut params = json!({"target": target(a), "dry_run": a.dry_run});
    if let Some(d) = a.positional.get(1) {
        if !matches!(
            d.as_str(),
            "left" | "right" | "up" | "down" | "next" | "previous"
        ) {
            return Err(CliError::usage(format!("invalid direction `{d}`"))
                .hint("left, right, up, down, next or previous"));
        }
        params["direction"] = json!(d);
    }
    require(endpoint)?.request(method, &params)
}

/// `window rename <window> <name>`.
pub fn window_rename(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let name = a.positional.get(1).map_or("", String::as_str);
    require(endpoint)?.request(
        "window.rename",
        &json!({"target": target(a), "name": name, "dry_run": a.dry_run}),
    )
}

/// `window kill <window>` and `pane kill <pane>`: shows the plan with `--dry-run`; otherwise
/// asks (or needs `--yes`), then ends exactly the confirmed window or pane **by id**, so a
/// renumbered index can never redirect it. The Host refuses the last running window or pane of
/// a Terminal: `subak kill <terminal>` ends a Terminal.
pub fn close(a: &Args, endpoint: &str, window: bool) -> Result<Value, CliError> {
    let method = if window { "window.kill" } else { "pane.kill" };
    let mut s = require(endpoint)?;
    let plan = s.request(
        method,
        &json!({"target": target(a), "dry_run": true, "force": true}),
    )?;
    if a.dry_run {
        return Ok(plan);
    }
    let p = &plan["would_kill"];
    let panes = p["panes"].as_array().map_or(&[][..], Vec::as_slice);
    let what = format!(
        "{} pane(s) with {} process(es) ({})",
        panes.len(),
        p["processes"],
        panes
            .iter()
            .map(|x| format!("pid {} {}", x["pid"], x["program"].as_str().unwrap_or("?")))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let (kind, id) = if window {
        ("window", p["window_id"].as_str().unwrap_or("?").to_string())
    } else {
        (
            "pane",
            panes
                .first()
                .and_then(|x| x["pane_id"].as_str())
                .unwrap_or("?")
                .to_string(),
        )
    };
    let name = p["name"].as_str().unwrap_or("?");
    confirm(
        a,
        &format!(
            "End {what} of {kind} {id} in Terminal `{name}` now? Every other pane keeps running."
        ),
        &format!("this ends {what} of {kind} {id} in Terminal `{name}` at once"),
        "re-run with --yes, or with --dry-run to see what would end",
    )?;
    s.request_within(
        method,
        &json!({"target": id, "force": true, "dry_run": false}),
        client::STOP_TIMEOUT,
    )
}

pub fn rename(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let name = a.positional.get(1).map_or("", String::as_str);
    require(endpoint)?.request(
        "terminal.rename",
        &json!({"target": target(a), "name": name, "dry_run": a.dry_run}),
    )
}

/// `attach [<target>]`: without a target, the Host's only running Terminal
/// (`ambiguous_target` with the candidates when there are several).
pub fn attach(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    if connect(endpoint)?.is_none() {
        return Err(host_not_running(endpoint));
    }
    client::attach(a, endpoint, a.positional.first().map(String::as_str))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Stop,
    Kill,
    Rm,
}

/// `stop | kill | rm <target>`: shows the plan with `--dry-run`; otherwise asks for consent (or
/// `--yes`) whenever processes would end or the Terminal would be removed.
pub fn end(a: &Args, endpoint: &str, cmd: Cmd) -> Result<Value, CliError> {
    use crate::cli::Cmd;
    let verb = match cmd {
        Cmd::Stop => Verb::Stop,
        Cmd::Kill => Verb::Kill,
        _ => Verb::Rm,
    };
    let method = match verb {
        Verb::Stop => "terminal.stop",
        Verb::Kill => "terminal.kill",
        Verb::Rm => "terminal.remove",
    };
    let mut s = require(endpoint)?;
    let t = s.request("terminal.info", &json!({"target": target(a)}))?["terminal"].clone();
    let force = verb != Verb::Rm || a.force;
    if a.dry_run {
        return s.request(
            method,
            &json!({"target": t["id"], "dry_run": true, "force": force}),
        );
    }
    let state = t["state"].as_str().unwrap_or("?");
    let live = matches!(state, "starting" | "running" | "stopping");
    let name = t["name"].as_str().unwrap_or("?");
    let id = t["id"].as_str().unwrap_or("?");
    let pane = &t["windows"][0]["panes"][0];
    let what = format!(
        "{} process(es), root pid {} ({})",
        pane["processes"],
        pane["pid"],
        pane["command"]["program"].as_str().unwrap_or("?")
    );
    match verb {
        Verb::Stop if !live => {} // nothing runs: a no-op, no consent needed
        Verb::Rm if live && !a.force => {
            return Err(CliError::new(
                Code::InvalidState,
                format!("Terminal `{name}` ({id}) is {state}; `rm` removes only Terminals that run nothing"),
            )
            .hint("stop it first with `subak stop`, or pass --force (with --yes) to end its processes and remove it"));
        }
        Verb::Stop => confirm(
            a,
            &format!("Stop Terminal `{name}` ({id}) and end {what}? It is kept as stopped."),
            &format!("stopping Terminal `{name}` ends {what}"),
            "re-run with --yes, or with --dry-run to see what would end",
        )?,
        Verb::Kill | Verb::Rm if live => confirm(
            a,
            &format!("End {what} of Terminal `{name}` ({id}) now and remove the Terminal?"),
            &format!("this ends {what} of Terminal `{name}` at once and removes it"),
            "re-run with --yes, or with --dry-run to see what would end",
        )?,
        Verb::Kill | Verb::Rm => confirm(
            a,
            &format!("Remove the {state} Terminal `{name}` ({id})?"),
            &format!("removing Terminal `{name}` deletes its record"),
            "re-run with --yes, or with --dry-run to see what would be removed",
        )?,
    }
    // Target the id the user confirmed, never a name that could meanwhile mean another Terminal.
    s.request_within(
        method,
        &json!({"target": id, "force": force, "dry_run": false}),
        client::STOP_TIMEOUT,
    )
}

/// Step 27: `subak claude status [target]` — the Claude Code sessions the Host knows of.
pub fn claude_status(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let target = a.positional.first().cloned();
    require(endpoint)?.request("claude.status", &json!({"target": target}))
}

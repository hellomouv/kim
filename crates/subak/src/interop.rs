//! Step 38 (ADR 0071): Subak as a provider of Kim interop capabilities.
//!
//! `subak interop invoke --json --no-input` reads one `kim.interop.request` from stdin and runs
//! one of the capabilities `capabilities --json` lists under `provides`:
//!
//! | Capability | Interface | Effect | Does |
//! | --- | --- | --- | --- |
//! | `terminal.list` | 1.0.0 | read | the Terminals (id, name, state); no Host is started |
//! | `terminal.open` | 1.0.0 | write | a new Terminal (default shell or a profile), Host on demand |
//! | `terminal.paste-path` | 1.0.0 | write | types a file's quoted path into a pane, without Enter |
//!
//! A dry run checks everything and changes nothing. The caller (Kim) enforces consent; Subak
//! still refuses what a capability does not allow (no program of the caller's choice, a path
//! that is not an existing file, text with line breaks).

use crate::cli::Args;
use crate::output::{CliError, Code};
use serde_json::{Value, json};
use std::io::Read;

/// The capabilities Subak provides: (id, interface version, effect, summary).
pub const PROVIDES: [(&str, &str, &str, &str); 3] = [
    (
        "terminal.list",
        "1.0.0",
        "read",
        "The Terminals of the user's Subak Host (id, name, state); starts nothing",
    ),
    (
        "terminal.open",
        "1.0.0",
        "write",
        "Open a new Terminal with the default shell or a profile (params: name, cwd, profile)",
    ),
    (
        "terminal.paste-path",
        "1.0.0",
        "write",
        "Type an existing file's quoted path into a pane, without Enter (params: target, path)",
    ),
];

fn param<'a>(p: &'a Value, k: &str) -> Result<Option<&'a str>, CliError> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.contains(['\0', '\r', '\n']) && s.len() <= 4096 => Ok(Some(s)),
        Some(_) => Err(CliError::usage(format!(
            "`{k}` must be a string without line breaks"
        ))),
    }
}

/// Checks the request; returns (capability, params, dry_run).
pub fn parse(text: &str) -> Result<(String, Value, bool), CliError> {
    if text.len() > 64 * 1024 {
        return Err(CliError::usage("the request is larger than 64 KiB"));
    }
    let req: Value = serde_json::from_str(text)
        .map_err(|e| CliError::usage(format!("the request on stdin is not JSON: {e}")))?;
    if req["schema"] != "kim.interop.request" || req["schema_version"] != 1 {
        return Err(CliError::new(
            Code::ToolIncompatible,
            "not a kim.interop.request version 1",
        ));
    }
    let cap = req["capability"].as_str().unwrap_or_default().to_string();
    let Some((_, version, _, _)) = PROVIDES.iter().find(|(id, ..)| *id == cap) else {
        return Err(CliError::new(
            Code::CapabilityMissing,
            format!("Subak does not provide `{cap}`"),
        )
        .hint("`subak capabilities --json` lists what it provides"));
    };
    let major = version.split('.').next().unwrap_or("1");
    if req["interface"].as_str() != Some(major) {
        return Err(CliError::new(
            Code::ToolIncompatible,
            format!(
                "Subak speaks `{cap}` interface {major}, not {}",
                req["interface"].as_str().unwrap_or("?")
            ),
        ));
    }
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(CliError::usage("`params` must be an object"));
    }
    Ok((cap, params, req["dry_run"] == true))
}

/// `subak interop invoke`: one request from stdin.
pub fn invoke(a: &Args, endpoint: &str) -> Result<Value, CliError> {
    let mut text = String::new();
    std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| CliError::new(Code::Io, format!("cannot read the request: {e}")))?;
    let (cap, p, dry) = parse(&text)?;
    let mut base = a.clone();
    base.positional.clear();
    base.command.clear();
    base.json = true;
    base.no_input = true;
    base.dry_run = dry;
    let out = match cap.as_str() {
        "terminal.list" => match crate::terminal::list(endpoint) {
            Ok(v) => {
                json!({"host_running": !v["host"].is_null(), "terminals": v["terminals"].as_array().map(|t| t.iter().map(|x| json!({"id": x["id"], "name": x["name"], "state": x["state"]})).collect::<Vec<_>>())})
            }
            Err(e) if e.code == Code::HostNotRunning => {
                json!({"host_running": false, "terminals": []})
            }
            Err(e) => return Err(e),
        },
        "terminal.open" => {
            base.positional = param(&p, "name")?.map(String::from).into_iter().collect();
            base.cwd = param(&p, "cwd")?.map(String::from);
            base.profile = param(&p, "profile")?.map(String::from);
            if p.get("command").is_some() || p.get("command_line").is_some() {
                return Err(CliError::usage(
                    "`terminal.open` starts a shell or a profile, never a program of the caller's choice",
                ));
            }
            let r = crate::terminal::new(&base, endpoint)?;
            if dry {
                json!({"dry_run": true, "would_create": r.get("would_create").cloned().unwrap_or(r)})
            } else {
                json!({"terminal_id": r["terminal_id"], "name": r["name"], "pane_id": r["pane_id"]})
            }
        }
        "terminal.paste-path" => {
            let target = param(&p, "target")?.ok_or_else(|| {
                CliError::usage("`target` (a Terminal, window or pane) is required")
            })?;
            let path = param(&p, "path")?.ok_or_else(|| CliError::usage("`path` is required"))?;
            let pb = std::path::Path::new(path);
            if !pb.is_absolute() || !pb.is_file() {
                return Err(CliError::new(
                    Code::Io,
                    format!("`{path}` is not an existing file (an absolute path)"),
                ));
            }
            if path.contains('"') {
                return Err(CliError::usage("a path with `\"` cannot be quoted safely"));
            }
            let text = format!("\"{path}\"");
            let mut s = crate::terminal::require(endpoint)?;
            let r = s.request(
                "pane.send_keys",
                &json!({"target": target, "keys": [text], "literal": true, "dry_run": dry}),
            )?;
            json!({"dry_run": dry, "typed": text, "pane": r.get("pane_id").cloned().or_else(|| r["would_send"].get("pane_id").cloned())})
        }
        _ => unreachable!("parse checked the capability"),
    };
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(cap: &str, iface: &str) -> String {
        json!({"schema": "kim.interop.request", "schema_version": 1, "capability": cap, "interface": iface, "params": {}, "dry_run": false}).to_string()
    }

    #[test]
    fn requests_are_checked_before_anything_runs() {
        assert_eq!(
            parse(&req("terminal.open", "1")).unwrap().0,
            "terminal.open"
        );
        assert_eq!(
            parse(&req("shell.exec", "1")).unwrap_err().code,
            Code::CapabilityMissing
        );
        assert_eq!(
            parse(&req("terminal.open", "2")).unwrap_err().code,
            Code::ToolIncompatible
        );
        assert_eq!(parse("{}").unwrap_err().code, Code::ToolIncompatible);
        assert_eq!(parse("not json").unwrap_err().code, Code::Usage);
        assert_eq!(parse(&"x".repeat(70_000)).unwrap_err().code, Code::Usage);
        // What Subak lists is what it serves.
        let caps: Value = serde_json::from_str(include_str!("capabilities.json")).unwrap();
        let listed: Vec<&str> = caps["provides"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["id"].as_str())
            .collect();
        let served: Vec<&str> = PROVIDES.iter().map(|p| p.0).collect();
        assert_eq!(listed, served);
    }
}

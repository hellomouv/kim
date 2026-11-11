//! Step 62 (ADR 0076, ADR 0071): Noeul as a provider of Kim interop capabilities.
//!
//! `noeul interop invoke --json --no-input` reads one `kim.interop.request` from stdin
//! (docs/INTEROP.md) and runs one capability of the running Noeul; it never starts Noeul and never
//! opens a window:
//!
//! | Capability | Interface | Effect | Does |
//! | --- | --- | --- | --- |
//! | `pin.list` | 1.0.0 | read | the open pins (ids, rectangles, zoom, visibility; no pixels) |
//! | `image.pin` | 1.0.0 | write | pins a local image file (`path`) without focus |
//! | `screen.capture-file` | 1.0.0 | write | captures a region or monitor to a PNG file (`region` or `monitor`, `output`, `replace`); `privacy.cli` applies (a notice, or refused) |
//!
//! The caller (Kim) enforces consent for `write`; Noeul still applies its own `[privacy]` and
//! path rules. Nothing reads the clipboard.

use crate::{Ctx, Outcome};
use noeul::error::{Code, NoeulError, Result};
use serde_json::{Value, json};
use std::io::Read;

/// (id, interface version, effect, summary) — also in `capabilities.json` under `provides`.
pub const PROVIDES: [(&str, &str, &str, &str); 3] = [
    (
        "pin.list",
        "1.0.0",
        "read",
        "The open pins of the running Noeul (ids, rectangles, zoom, visibility; no pixels); starts nothing",
    ),
    (
        "image.pin",
        "1.0.0",
        "write",
        "Pin a local image file in the running Noeul without focus (params: path)",
    ),
    (
        "screen.capture-file",
        "1.0.0",
        "write",
        "Capture a region or monitor to a PNG file (params: region or monitor, output, replace); Noeul's privacy.cli applies",
    ),
];

/// Checks the request; returns (capability, params, dry_run).
pub fn parse(text: &str) -> Result<(String, Value, bool)> {
    if text.len() > 64 * 1024 {
        return Err(NoeulError::new(
            Code::Usage,
            "the request is larger than 64 KiB",
        ));
    }
    let req: Value = serde_json::from_str(text).map_err(|e| {
        NoeulError::new(
            Code::Usage,
            format!("the request on stdin is not JSON: {e}"),
        )
    })?;
    if req["schema"] != "kim.interop.request" || req["schema_version"] != 1 {
        return Err(NoeulError::new(
            Code::ToolIncompatible,
            "not a kim.interop.request version 1",
        ));
    }
    let cap = req["capability"].as_str().unwrap_or_default().to_string();
    let Some((_, version, _, _)) = PROVIDES.iter().find(|(id, ..)| *id == cap) else {
        return Err(NoeulError::new(
            Code::CapabilityMissing,
            format!("Noeul does not provide `{cap}`"),
        )
        .hint("`noeul capabilities --json` lists what it provides"));
    };
    let major = version.split('.').next().unwrap_or("1");
    if req["interface"].as_str() != Some(major) {
        return Err(NoeulError::new(
            Code::ToolIncompatible,
            format!(
                "Noeul speaks `{cap}` interface {major}, not {}",
                req["interface"].as_str().unwrap_or("?")
            ),
        ));
    }
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(NoeulError::new(Code::Usage, "`params` must be an object"));
    }
    Ok((cap, params, req["dry_run"] == true))
}

/// The IPC request for a capability.
pub fn request_for(cap: &str, p: &Value, dry_run: bool) -> Result<(&'static str, Value)> {
    let usage = |m: &str| NoeulError::new(Code::Usage, m.to_string());
    match cap {
        "pin.list" => Ok(("pin.list", json!({}))),
        "image.pin" => {
            let path = p["path"]
                .as_str()
                .ok_or_else(|| usage("`path` is required"))?;
            Ok((
                "pin.create",
                json!({"source": "file", "path": path, "dry_run": dry_run}),
            ))
        }
        "screen.capture-file" => {
            let output = p["output"]
                .as_str()
                .ok_or_else(|| usage("`output` is required"))?;
            let mut r =
                json!({"output": output, "replace": p["replace"] == true, "dry_run": dry_run});
            match (p.get("region"), p.get("monitor")) {
                (Some(x), None) => r["region"] = x.clone(),
                (None, Some(m)) => r["monitor"] = m.clone(),
                _ => return Err(usage("give exactly one of `region` or `monitor`")),
            }
            Ok(("capture", r))
        }
        other => Err(NoeulError::new(
            Code::CapabilityMissing,
            format!("Noeul does not provide `{other}`"),
        )),
    }
}

/// `noeul interop invoke`: one request from stdin.
pub fn invoke(ctx: &Ctx, force_dry_run: bool) -> Result<Outcome> {
    let mut text = String::new();
    std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| NoeulError::new(Code::Io, format!("cannot read the request: {e}")))?;
    let (cap, params, dry) = parse(&text)?;
    let dry = dry || force_dry_run;
    let (method, p) = request_for(&cap, &params, dry)?;
    #[cfg(windows)]
    {
        let mut s = crate::instance::connect(ctx)?.ok_or_else(NoeulError::not_running)?;
        let v = s.request(method, p)?;
        Ok(Outcome::new(v, format!("{cap}: done.\n")))
    }
    #[cfg(not(windows))]
    {
        let _ = (ctx, method, p);
        Err(NoeulError::unsupported_platform("the Noeul instance"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(cap: &str, interface: &str, params: &Value) -> String {
        json!({"schema": "kim.interop.request", "schema_version": 1, "capability": cap,
            "interface": interface, "params": params, "dry_run": false,
            "caller": {"tool": "kim", "version": "0.0.1"}})
        .to_string()
    }

    #[test]
    fn requests_are_checked_and_mapped_to_the_instance() {
        let (cap, p, dry) = parse(&req("image.pin", "1", &json!({"path": "C:\\\\a.png"}))).unwrap();
        assert_eq!((cap.as_str(), dry), ("image.pin", false));
        let (m, r) = request_for(&cap, &p, dry).unwrap();
        assert_eq!((m, r["source"].as_str()), ("pin.create", Some("file")));
        assert_eq!(
            parse(&req("image.pin", "2", &json!({}))).unwrap_err().code,
            Code::ToolIncompatible
        );
        assert_eq!(
            parse(&req("clipboard.read", "1", &json!({})))
                .unwrap_err()
                .code,
            Code::CapabilityMissing
        );
        assert_eq!(parse("{}").unwrap_err().code, Code::ToolIncompatible);
        let (m, r) = request_for(
            "screen.capture-file",
            &json!({"monitor": 1, "output": "C:\\\\s.png"}),
            true,
        )
        .unwrap();
        assert_eq!(
            (m, r["monitor"].as_i64(), r["dry_run"].as_bool()),
            ("capture", Some(1), Some(true))
        );
        assert!(r.get("via").is_none(), "a CLI caller: privacy.cli applies");
        assert!(
            request_for(
                "screen.capture-file",
                &json!({"output": "C:\\\\s.png"}),
                false
            )
            .is_err()
        );
        // The document's `provides` and this table agree.
        let caps: Value = serde_json::from_str(crate::CAPABILITIES).unwrap();
        let listed: Vec<(String, String, String)> = caps["provides"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["id"].as_str().unwrap().into(),
                    p["version"].as_str().unwrap().into(),
                    p["effect"].as_str().unwrap().into(),
                )
            })
            .collect();
        let ours: Vec<(String, String, String)> = PROVIDES
            .iter()
            .map(|(i, v, e, _)| ((*i).into(), (*v).into(), (*e).into()))
            .collect();
        assert_eq!(listed, ours);
    }
}

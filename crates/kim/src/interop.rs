//! Step 38 (ADR 0071): capability-based interoperability between independently installed Kim
//! tools, without a resident orchestrator (docs/INTEROP.md, CLI_CONTRACT §9.8).
//!
//! - A tool **provides** capabilities: stable dotted ids with their own SemVer interface version
//!   (`terminal.open` 1.0.0), listed in its capabilities document (`provides`), so a caller finds
//!   them by reading manifests — nothing is started to discover.
//! - A caller **invokes** one by running `<tool> interop invoke --json --no-input` with a
//!   `kim.interop.request` document on stdin: headless, no window, a time limit, capped output.
//!   The provider answers with its usual `--json` envelope.
//! - **Consent per effect** (CLI_CONTRACT §9.6): `read` any time; `write` only with an explicit
//!   user action (`--allow-write`); `destructive` also `--yes`; `install` never through interop;
//!   a capability that opens a window never from a headless call (`--allow-window`). A dry run
//!   changes nothing and needs no consent.

use crate::capabilities::{self, Capabilities, Effect};
use crate::error::{ErrorCode, KimError, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// `schema` of an interop request.
pub const REQUEST_SCHEMA: &str = "kim.interop.request";
/// Version of the request format.
pub const REQUEST_VERSION: u32 = 1;
/// Largest request a caller sends (and a provider accepts).
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
/// Largest answer a caller reads.
pub const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
/// Default time limit of an invocation.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// One capability a tool provides (`provides` of its capabilities document).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provides {
    /// Stable dotted id, e.g. `terminal.open`.
    pub id: String,
    /// SemVer of the capability's interface (not of the tool).
    pub version: String,
    pub summary: String,
    /// The strongest effect of the capability.
    pub effect: Effect,
    /// May use the network.
    #[serde(default)]
    pub network: bool,
    /// Opens a window: never invoked from a headless path.
    #[serde(default)]
    pub opens_window: bool,
}

/// Whether `id` is a capability id: dotted lowercase words, at least two.
pub fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id.split('.').count() >= 2
        && id.split('.').all(|w| {
            !w.is_empty()
                && w.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// What the user allowed this call to do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Consent {
    pub allow_write: bool,
    pub yes: bool,
    pub allow_window: bool,
    pub dry_run: bool,
}

/// Refuses a call the user did not allow (CLI_CONTRACT §9.6).
pub fn check_consent(p: &Provides, tool: &str, c: Consent) -> Result<()> {
    let refuse = |m: String, hint: &str| {
        Err(KimError::new(ErrorCode::ConfirmationRequired, m).with_hint(hint))
    };
    if p.opens_window && !c.allow_window {
        return refuse(
            format!(
                "{tool}'s `{}` opens a window; a headless call never does",
                p.id
            ),
            "pass --allow-window when the user asked for that window",
        );
    }
    if c.dry_run {
        return Ok(());
    }
    match p.effect {
        Effect::Read => Ok(()),
        Effect::Write if c.allow_write => Ok(()),
        Effect::Write => refuse(
            format!(
                "{tool}'s `{}` changes something; it runs only when the user asked",
                p.id
            ),
            "pass --allow-write for an explicit user action, or --dry-run to see what it would do",
        ),
        Effect::Install => Err(KimError::new(
            ErrorCode::ConfirmationRequired,
            format!(
                "{tool}'s `{}` installs software; Kim never installs through interop",
                p.id
            ),
        )
        .with_hint("install explicitly with `kim install`")),
        Effect::Destructive | Effect::Unknown if c.allow_write && c.yes => Ok(()),
        Effect::Destructive | Effect::Unknown => refuse(
            format!("{tool}'s `{}` is destructive", p.id),
            "it runs only after the user confirmed: pass --allow-write and --yes",
        ),
    }
}

/// One provider of a capability among the installed tools.
#[derive(Debug, Clone, Serialize)]
pub struct Provider {
    pub tool: String,
    pub tool_version: String,
    pub executable: String,
    pub capability: Provides,
    /// The executable exists and the document was readable.
    pub available: bool,
    /// The interface version satisfies the caller's requirement (always true without one).
    pub compatible: bool,
}

/// The providers of `id` in `docs` (tool, version, executable, capabilities).
pub fn providers(
    id: &str,
    docs: &[(String, String, std::path::PathBuf, Capabilities)],
    req: Option<&semver::VersionReq>,
) -> Vec<Provider> {
    let mut out = Vec::new();
    for (tool, version, exe, caps) in docs {
        for p in caps.provides.iter().filter(|p| p.id == id) {
            let compatible =
                req.is_none_or(|r| semver::Version::parse(&p.version).is_ok_and(|v| r.matches(&v)));
            out.push(Provider {
                tool: tool.clone(),
                tool_version: version.clone(),
                executable: exe.display().to_string(),
                capability: p.clone(),
                available: exe.is_file(),
                compatible,
            });
        }
    }
    out
}

/// The request document for `capability` (the caller's interface major, the params).
pub fn request(
    capability: &str,
    interface: &str,
    params: &serde_json::Value,
    dry_run: bool,
) -> serde_json::Value {
    serde_json::json!({
        "schema": REQUEST_SCHEMA,
        "schema_version": REQUEST_VERSION,
        "capability": capability,
        "interface": interface,
        "params": params,
        "dry_run": dry_run,
        "caller": {"tool": "kim", "version": crate::KIM_VERSION},
    })
}

/// Runs `<exe> interop invoke --json --no-input` with `input` on stdin, bounded by `timeout`;
/// returns the provider's `data`, or its error as `tool_failed` with its code in `details`.
pub fn invoke(
    exe: &Path,
    tool: &str,
    input: &serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let body =
        serde_json::to_vec(input).map_err(|e| KimError::new(ErrorCode::Internal, e.to_string()))?;
    if body.len() > MAX_REQUEST_BYTES {
        return Err(KimError::new(
            ErrorCode::Usage,
            format!("the request is larger than {MAX_REQUEST_BYTES} bytes"),
        ));
    }
    let mut cmd = Command::new(exe);
    cmd.args(["interop", "invoke", "--json", "--no-input"])
        .env("KIM_NO_INPUT", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| {
        if exe.exists() {
            KimError::new(
                ErrorCode::Io,
                format!("cannot start {}: {e}", exe.display()),
            )
        } else {
            KimError::new(
                ErrorCode::NotInstalled,
                format!("{tool}'s program {} is missing", exe.display()),
            )
            .with_hint(format!(
                "repair it explicitly with `kim install {tool} --reinstall`"
            ))
        }
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        // A provider that does not read its input may close the pipe: that is its answer.
        let _ = stdin.write_all(&body);
    }
    let stdout = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout {
            let _ = out.take(MAX_RESPONSE_BYTES + 1).read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(KimError::new(
                    ErrorCode::Timeout,
                    format!(
                        "{tool} did not answer within {} s and was stopped",
                        timeout.as_secs()
                    ),
                )
                .with_hint("retry with a longer --timeout, or check the tool"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(KimError::new(ErrorCode::Io, format!("{tool}: {e}"))),
        }
    }
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(200));
    let out = rx.recv_timeout(remaining).unwrap_or_default();
    if out.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(KimError::new(
            ErrorCode::ToolFailed,
            format!("{tool} answered with more than {MAX_RESPONSE_BYTES} bytes"),
        ));
    }
    let failed = |m: String| KimError::new(ErrorCode::ToolFailed, m);
    let env: serde_json::Value = serde_json::from_slice(&out)
        .map_err(|_| failed(format!("{tool} did not answer with a JSON envelope")).with_hint("this version of the tool may not implement `interop invoke`; check `kim tools describe`"))?;
    if env
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(u64::from(capabilities::JSON_SCHEMA_VERSION))
    {
        return Err(KimError::new(
            ErrorCode::ToolIncompatible,
            format!("{tool} answered with an envelope this Kim cannot read"),
        ));
    }
    if env.get("ok") == Some(&serde_json::Value::Bool(true)) {
        return Ok(env.get("data").cloned().unwrap_or_default());
    }
    let code = env
        .pointer("/error/code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let msg = env
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    // The provider's refusals of the protocol itself keep their meaning.
    let ours = match code.as_str() {
        "capability_missing" => Some(ErrorCode::CapabilityMissing),
        "tool_incompatible" => Some(ErrorCode::ToolIncompatible),
        _ => None,
    };
    let mut e = KimError::new(
        ours.unwrap_or(ErrorCode::ToolFailed),
        format!("{tool}: {}", crate::redact::foreign(&msg)),
    )
    .with_detail(format!("tool_code: {code}"));
    if let Some(h) = env
        .pointer("/error/hint")
        .and_then(serde_json::Value::as_str)
    {
        e = e.with_hint(crate::redact::foreign(h));
    }
    Err(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(effect: Effect, window: bool) -> Provides {
        Provides {
            id: "x.y".into(),
            version: "1.0.0".into(),
            summary: "s".into(),
            effect,
            network: false,
            opens_window: window,
        }
    }

    #[test]
    fn capability_ids_are_dotted_lowercase_words() {
        for ok in ["terminal.open", "capture.region-to-file", "a.b.c2"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in [
            "terminal",
            "Terminal.open",
            ".open",
            "a..b",
            "a.b c",
            "a.2b",
            "",
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn consent_follows_the_effect() {
        let c = Consent::default();
        assert!(check_consent(&p(Effect::Read, false), "t", c).is_ok());
        assert!(check_consent(&p(Effect::Write, false), "t", c).is_err());
        let w = Consent {
            allow_write: true,
            ..c
        };
        assert!(check_consent(&p(Effect::Write, false), "t", w).is_ok());
        assert!(check_consent(&p(Effect::Destructive, false), "t", w).is_err());
        let wy = Consent { yes: true, ..w };
        assert!(check_consent(&p(Effect::Destructive, false), "t", wy).is_ok());
        assert!(
            check_consent(&p(Effect::Unknown, false), "t", w).is_err(),
            "unknown is destructive"
        );
        assert!(
            check_consent(&p(Effect::Install, false), "t", wy).is_err(),
            "never installs"
        );
        let dry = Consent { dry_run: true, ..c };
        assert!(check_consent(&p(Effect::Destructive, false), "t", dry).is_ok());
        assert!(
            check_consent(&p(Effect::Read, true), "t", dry).is_err(),
            "a window never headless"
        );
        let win = Consent {
            allow_window: true,
            ..c
        };
        assert!(check_consent(&p(Effect::Read, true), "t", win).is_ok());
        let e = check_consent(&p(Effect::Write, false), "t", c).unwrap_err();
        assert_eq!(e.code, ErrorCode::ConfirmationRequired);
    }
}

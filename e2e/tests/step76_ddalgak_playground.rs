//! E2E — Step 76: Ddalgak's test playground from the command line — `ddalgak test` simulates
//! one key event against the saved rules or the draft (docs/DDALGAK_RULE_MODEL.md §11, ADR 0077).
//! Requirements: `DDALGAK-FR-040`, `DDALGAK-FR-041`, `DDALGAK-FR-042`, `DDALGAK-SEC-001`,
//! `KIM-FR-012`.
//!
//! Scenario (release `ddalgak`, isolated profile, no app): an agent tests a profile whose rules
//! cover all seven action categories (remap, type text, open a program, open a website, move the
//! pointer, change the mode, control the engine). Each test names the winner, every candidate and
//! a contract-shaped preview of every step; it is always `simulate` with no injected event and
//! nothing opened; typed text, the URL's path and query and the program's folder never appear.
//! The context changes the decision: another mode, a paused engine, a password field, an IME
//! composition, an injected event, the secure desktop and a draft that is not saved yet. A
//! misconfigured key is a usage error. The rules file is unchanged afterwards.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`; no app is started, no key is injected,
//! the program (which does not exist) and the website are never opened; no window appears.
#![cfg(windows)]

#[path = "../../xtask/src/jsonschema.rs"]
mod jsonschema;

use kim_e2e::{TempDir, bin, require_e2e};
use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TEXT: &str = "Step76-비밀-문장";
const GRANT: &str = "2026-11-04T09:00:00Z";

fn schema(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../docs/contracts/schemas/{name}.schema.json"));
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

#[allow(clippy::needless_pass_by_value)]
fn launch(id: &str, keys: &str, step: Value) -> Value {
    let mut r = json!({ "id": id, "title": id, "enabled": true, "trigger": { "keys": keys },
        "steps": [step] });
    let fp = ddalgak::validate::launch_fingerprint(&r);
    r["consent"] =
        json!({ "launch-programs": { "granted_at": GRANT, "via": "gui", "fingerprint": fp } });
    r
}

fn rules() -> Value {
    let inject = json!({ "input-injection": { "granted_at": GRANT, "via": "gui" } });
    json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 4,
        "active_profile": "work",
        "profiles": [ { "id": "work", "title": "Work", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" }, { "id": "nav", "title": "Navigation" } ],
            "rules": [
                { "id": "caps", "title": "Caps", "enabled": true, "trigger": { "keys": "CapsLock" },
                  "steps": [ { "type": "remap", "to": "LCtrl", "tap": "Escape" } ], "consent": inject },
                { "id": "sig", "title": "Sig", "enabled": true, "trigger": { "keys": "Ctrl+Alt+Shift+F21" },
                  "steps": [ { "type": "type_text", "text": TEXT } ], "consent": inject },
                launch("tool", "Ctrl+Alt+Shift+F22",
                    json!({ "type": "open_program", "path": "C:\\Kim-E2E-Step76\\never-started.exe" })),
                launch("site", "Ctrl+Alt+Shift+F23",
                    json!({ "type": "open_url", "url": "https://example.com/private/path?token=Step76" })),
                { "id": "left", "title": "Left", "enabled": true, "modes": ["nav"],
                  "trigger": { "keys": "Ctrl+Alt+Shift+F24" },
                  "steps": [ { "type": "mouse_move", "mode": "relative", "dx": -200, "dy": 0 } ],
                  "consent": inject },
                { "id": "to-nav", "title": "Nav", "enabled": true, "trigger": { "keys": "Ctrl+Alt+Shift+F20" },
                  "steps": [ { "type": "mode", "op": "toggle", "mode": "nav" } ] },
                { "id": "pause", "title": "Pause", "enabled": true, "trigger": { "keys": "Ctrl+Alt+Shift+F19" },
                  "steps": [ { "type": "engine", "op": "toggle_pause" } ] }
            ] } ]
    })
}

struct Env<'a> {
    sb: &'a TempDir,
}

impl Env<'_> {
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let p = self.sb.path();
        let out = Command::new(bin("ddalgak"))
            .args(args)
            .arg("--json")
            .env("APPDATA", p.join("Roaming"))
            .env("LOCALAPPDATA", p.join("Local"))
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e})"));
        let errors = jsonschema::Validator::new(&schema("ddalgak-cli-v1"))
            .validate(&doc)
            .unwrap();
        assert!(errors.is_empty(), "{args:?}: {errors:?}\n{doc:#}");
        assert!(out.stderr.is_empty(), "{args:?}: stderr with --json");
        let text = doc.to_string();
        for secret in [TEXT, "private", "token=", "Kim-E2E-Step76"] {
            assert!(
                !text.contains(secret),
                "{args:?}: `{secret}` appeared\n{doc:#}"
            );
        }
        (out.status.code().unwrap_or(-1), doc)
    }

    /// A test that succeeds; its data validates against ddalgak-simulation-v1 and did nothing.
    fn test(&self, args: &[&str]) -> Value {
        let mut a = vec!["test"];
        a.extend_from_slice(args);
        let (code, d) = self.run(&a);
        assert_eq!(code, 0, "{args:?}: {d:#}");
        let data = d["data"].clone();
        let errors = jsonschema::Validator::new(&schema("ddalgak-simulation-v1"))
            .validate(&data)
            .unwrap();
        assert!(errors.is_empty(), "{args:?}: {errors:?}");
        assert_eq!(
            (data["kind"].as_str(), data["injected_events"].as_u64()),
            (Some("simulate"), Some(0))
        );
        assert_eq!(data["opened"], json!([]));
        assert!(
            data["effects"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["performed"] == false)
        );
        data
    }
}

fn fired(d: &Value, rule: &str, ty: &str) -> Value {
    assert_eq!(
        (
            d["decision"]["outcome"].as_str(),
            d["decision"]["rule"].as_str()
        ),
        (Some("fired"), Some(rule)),
        "{d:#}"
    );
    assert_eq!(d["effects"][0]["type"], ty, "{d:#}");
    d["effects"][0]["preview"].clone()
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step76_test_every_action_category_without_side_effects() {
    require_e2e();
    let sb = TempDir::new("step76-test");
    let config = sb.path().join("Roaming/mouv/ddalgak");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    std::fs::write(
        config.join("rules.json"),
        serde_json::to_vec_pretty(&rules()).unwrap(),
    )
    .unwrap();
    let before = std::fs::read(config.join("rules.json")).unwrap();
    let e = Env { sb: &sb };

    // The seven categories.
    let p = fired(&e.test(&["CapsLock"]), "caps", "remap");
    assert_eq!(
        (p["holds"].as_str(), p["tap"].as_str()),
        (Some("LCtrl"), Some("Escape"))
    );
    let p = fired(&e.test(&["Ctrl+Alt+Shift+F21"]), "sig", "type_text");
    assert_eq!(
        (p["characters"].as_u64(), p["lines"].as_u64()),
        (Some(TEXT.chars().count() as u64), Some(1))
    );
    let p = fired(&e.test(&["Ctrl+Alt+Shift+F22"]), "tool", "open_program");
    assert_eq!(
        (p["program"].as_str(), p["found"].as_bool()),
        (Some("never-started.exe"), Some(false))
    );
    let p = fired(&e.test(&["Ctrl+Alt+Shift+F23"]), "site", "open_url");
    assert_eq!(p, json!({ "scheme": "https", "host": "example.com" }));
    let p = fired(
        &e.test(&["Ctrl+Alt+Shift+F24", "--mode", "nav"]),
        "left",
        "mouse_move",
    );
    assert_eq!(
        p["to"],
        json!({ "x": 760, "y": 540 }),
        "from the simulated centre, 200 px left"
    );
    let p = fired(&e.test(&["Ctrl+Alt+Shift+F20"]), "to-nav", "mode");
    assert_eq!(
        (p["from"].as_str(), p["to"].as_str()),
        (Some("base"), Some("nav"))
    );
    let p = fired(
        &e.test(&["Ctrl+Alt+Shift+F19", "--paused"]),
        "pause",
        "engine",
    );
    assert_eq!(
        (p["from"].as_str(), p["to"].as_str()),
        (Some("paused"), Some("running")),
        "engine rules run while paused"
    );

    // The context decides.
    let d = e.test(&["Ctrl+Alt+Shift+F24"]);
    assert_eq!(
        (
            d["decision"]["outcome"].as_str(),
            d["decision"]["candidates"][0]["result"].as_str()
        ),
        (Some("no_match"), Some("mode_mismatch"))
    );
    for (flags, code) in [
        (&["--paused"][..], "skipped_paused"),
        (&["--focus", "password_field"][..], "skipped_password_field"),
        (&["--ime"][..], "skipped_ime_composition"),
        (&["--injected"][..], "skipped_injected_input"),
        (&["--focus", "secure_desktop"][..], "skipped_secure_desktop"),
    ] {
        let mut a = vec!["Ctrl+Alt+Shift+F21"];
        a.extend_from_slice(flags);
        let d = e.test(&a);
        assert_eq!(
            (
                d["decision"]["code"].as_str(),
                d["decision"]["key_reaches_app"].as_bool()
            ),
            (Some(code), Some(true)),
            "{flags:?}"
        );
    }
    assert_eq!(e.test(&["F18"])["decision"]["outcome"], "no_match");

    // A misconfigured key, a bad option and a missing draft are refused in words.
    let (code, d) = e.run(&["test", "Ctrl+Nope"]);
    assert_eq!((code, d["error"]["code"].as_str()), (2, Some("usage")));
    let (code, _) = e.run(&["test", "F18", "--focus", "fullscreen"]);
    assert_eq!(code, 2);
    let (_, d) = e.run(&["test", "F18", "--draft"]);
    assert_eq!(d["error"]["code"], "snapshot_not_found");

    // An unsaved draft is tested before it is applied; the saved rules stay as they were.
    let (code, d) = e.run(&[
        "rule",
        "edit",
        "sig",
        "--trigger",
        "Ctrl+Alt+Shift+F17",
        "--draft",
    ]);
    assert_eq!(code, 0, "{d:#}");
    let d = e.test(&["Ctrl+Alt+Shift+F17", "--draft"]);
    assert_eq!(
        (d["decision"]["rule"].as_str(), d["revision"].as_u64()),
        (Some("sig"), None)
    );
    assert_eq!(
        e.test(&["Ctrl+Alt+Shift+F17"])["decision"]["outcome"],
        "no_match"
    );
    let (code, _) = e.run(&["rules", "draft", "discard", "--yes"]);
    assert_eq!(code, 0);

    assert_eq!(
        std::fs::read(config.join("rules.json")).unwrap(),
        before,
        "the rules file is unchanged"
    );
}

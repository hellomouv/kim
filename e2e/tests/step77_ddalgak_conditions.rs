//! E2E — Step 77: Ddalgak's app and window conditions (docs/DDALGAK_RULE_MODEL.md §7.1–§7.2,
//! ADR 0078). Requirements: `DDALGAK-FR-022`, `DDALGAK-FR-023`, `DDALGAK-FR-040`,
//! `DDALGAK-SEC-011`, `KIM-FR-012`.
//!
//! Scenario A (release `ddalgak`, isolated profile, no app): one key bound three ways — in
//! Code, in Excel and everywhere else — plus an `except_apps` rule given by full path, a
//! title rule and a rule with a condition this version does not know. `ddalgak explain
//! --context` names the winner for each app with every candidate's result; an unknown app
//! (an administrator window, the sign-in screen) never matches a condition, so the global rule
//! wins; only the unknown condition stays inactive (`condition_unsupported`); a bad context is a
//! usage error. Nothing is injected, opened or changed.
//!
//! Scenario B (debug test host of `ddalgak-gui`: no tray icon, a hook that never consumes a
//! key): the running engine loads the app and title rules as active — only the unknown condition
//! is inactive — and its status carries no window title or program path.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`, own pipe endpoint; the focus is never
//! moved, no key is sent; only the test's own host is killed; no window or tray icon appears.
#![cfg(windows)]

#[path = "../../xtask/src/jsonschema.rs"]
mod jsonschema;

use kim_e2e::{TempDir, bin, bin_debug, require_e2e};
use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn schema(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../docs/contracts/schemas/{name}.schema.json"));
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn engine(id: &str, keys: &str, when: Option<Value>) -> Value {
    let mut r = json!({ "id": id, "title": id, "enabled": true, "trigger": { "keys": keys },
        "steps": [ { "type": "engine", "op": "toggle_pause" } ] });
    if let Some(w) = when {
        r["when"] = w;
    }
    r
}

fn rules() -> Value {
    json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 2,
        "active_profile": "work", "settings": { "engine_on_start": "on" },
        "profiles": [ { "id": "work", "title": "Work", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" } ],
            "rules": [
                engine("global", "Ctrl+Alt+Shift+F21", None),
                engine("in-code", "Ctrl+Alt+Shift+F21", Some(json!({ "apps": [ { "exe": "Code.exe" } ] }))),
                engine("in-excel", "Ctrl+Alt+Shift+F21", Some(json!({ "apps": [ { "exe": "EXCEL.EXE" } ] }))),
                engine("not-vault", "Ctrl+Alt+Shift+F22", Some(json!({ "except_apps": [
                    { "path": "C:\\Program Files\\KeePassXC\\KeePassXC.exe" } ] }))),
                engine("meeting", "Ctrl+Alt+Shift+F23", Some(json!({ "apps": [ { "exe": "ms-teams.exe" } ],
                    "window_title": { "contains": "Meeting" } }))),
                engine("newer", "Ctrl+Alt+Shift+F24", Some(json!({ "battery": "charging" })))
            ] } ]
    })
}

struct Env<'a> {
    sb: &'a TempDir,
    endpoint: String,
}

impl Env<'_> {
    fn cmd(&self, exe: &Path) -> Command {
        let mut c = Command::new(exe);
        let p = self.sb.path();
        c.env("APPDATA", p.join("Roaming"))
            .env("LOCALAPPDATA", p.join("Local"))
            .creation_flags(CREATE_NO_WINDOW);
        c
    }

    fn run(&self, args: &[&str]) -> (i32, Value) {
        let out = self
            .cmd(&bin("ddalgak"))
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e})"));
        let errors = jsonschema::Validator::new(&schema("ddalgak-cli-v1"))
            .validate(&doc)
            .unwrap();
        assert!(errors.is_empty(), "{args:?}: {errors:?}\n{doc:#}");
        assert!(out.stderr.is_empty(), "{args:?}: stderr with --json");
        (out.status.code().unwrap_or(-1), doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, d) = self.run(args);
        assert_eq!(code, 0, "{args:?}: {d:#}");
        d["data"].clone()
    }

    /// `explain` in a context; returns (winner, [(rule, result)]).
    fn explain(&self, keys: &str, context: &Value) -> (Option<String>, Vec<(String, String)>) {
        let d = self.ok(&["explain", keys, "--context", &context.to_string()]);
        let errors = jsonschema::Validator::new(&schema("ddalgak-simulation-v1"))
            .validate(&d)
            .unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            (d["kind"].as_str(), d["injected_events"].as_u64()),
            (Some("simulate"), Some(0))
        );
        let c = d["decision"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["rule"].as_str().unwrap().to_string(),
                    c["result"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        (d["decision"]["rule"].as_str().map(String::from), c)
    }

    fn config(&self) -> PathBuf {
        self.sb.path().join("Roaming/mouv/ddalgak")
    }

    fn write_rules(&self) {
        std::fs::create_dir_all(self.config()).unwrap();
        std::fs::create_dir_all(self.sb.path().join("Local")).unwrap();
        std::fs::write(self.config().join("rules.json"), rules().to_string()).unwrap();
    }
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step77_the_same_key_per_app_with_explanations_and_safe_failure() {
    require_e2e();
    let sb = TempDir::new("step77-cli");
    let e = Env {
        sb: &sb,
        endpoint: String::new(),
    };
    e.write_rules();
    let before = std::fs::read(e.config().join("rules.json")).unwrap();

    let (w, c) = e.explain(
        "Ctrl+Alt+Shift+F21",
        &json!({ "app": { "exe": "Code.exe" } }),
    );
    assert_eq!(w.as_deref(), Some("in-code"));
    assert_eq!(
        c,
        pairs(&[
            ("in-code", "winner"),
            ("in-excel", "condition_false"),
            ("global", "lower_precedence")
        ])
    );
    let (w, _) = e.explain(
        "Ctrl+Alt+Shift+F21",
        &json!({ "app": { "exe": "C:\\Program Files\\Microsoft Office\\root\\Office16\\EXCEL.EXE" } }),
    );
    assert_eq!(
        w.as_deref(),
        Some("in-excel"),
        "a full path matches by file name"
    );
    let (w, _) = e.explain(
        "Ctrl+Alt+Shift+F21",
        &json!({ "app": { "exe": "notepad.exe" } }),
    );
    assert_eq!(w.as_deref(), Some("global"));
    // Unknown (no readable app): no condition matches; the global rule takes the key.
    let (w, c) = e.explain("Ctrl+Alt+Shift+F21", &json!({ "app": null }));
    assert_eq!(w.as_deref(), Some("global"));
    assert!(
        c.iter()
            .filter(|(r, _)| r != "global")
            .all(|(_, x)| x == "condition_false"),
        "{c:?}"
    );
    // except_apps by full path, and unknown never passes it either.
    let vault = json!({ "app": { "exe": "C:\\Program Files\\KeePassXC\\KeePassXC.exe" } });
    assert_eq!(e.explain("Ctrl+Alt+Shift+F22", &vault).0, None);
    assert_eq!(
        e.explain(
            "Ctrl+Alt+Shift+F22",
            &json!({ "app": { "exe": "Code.exe" } })
        )
        .0
        .as_deref(),
        Some("not-vault")
    );
    assert_eq!(
        e.explain("Ctrl+Alt+Shift+F22", &json!({ "app": null })).0,
        None
    );
    // A title condition with its app.
    let teams = |t: &str| json!({ "app": { "exe": "ms-teams.exe", "window_title": t } });
    assert_eq!(
        e.explain("Ctrl+Alt+Shift+F23", &teams("Weekly MEETING | Teams"))
            .0
            .as_deref(),
        Some("meeting")
    );
    assert_eq!(
        e.explain("Ctrl+Alt+Shift+F23", &teams("Chat | Teams")).0,
        None
    );
    // An unknown condition stays inactive, never unconditional.
    let d = e.ok(&["explain", "Ctrl+Alt+Shift+F24"]);
    assert_eq!(
        (
            d["decision"]["outcome"].as_str(),
            d["decision"]["code"].as_str()
        ),
        (Some("skipped"), Some("condition_unsupported"))
    );
    let check = e.ok(&["rule", "check"]);
    let codes: Vec<(String, String)> = check["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["rule"].as_str().unwrap_or("").to_string(),
                p["code"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        pairs(&[("newer", "condition_unsupported")]),
        "{check:#}"
    );
    // A bad context is a usage error.
    for bad in [r#"{"window":"x"}"#, "not json", r#"{"focus":"fullscreen"}"#] {
        let (code, d) = e.run(&["explain", "Ctrl+Alt+Shift+F21", "--context", bad]);
        assert_eq!(
            (code, d["error"]["code"].as_str()),
            (2, Some("usage")),
            "{bad}"
        );
    }
    assert_eq!(
        std::fs::read(e.config().join("rules.json")).unwrap(),
        before
    );
}

struct Host(Child);

impl Drop for Host {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step77_the_running_engine_loads_conditioned_rules_as_active() {
    require_e2e();
    let sb = TempDir::new("step77-app");
    let e = Env {
        sb: &sb,
        endpoint: format!("kim-e2e-step77-{}", std::process::id()),
    };
    e.write_rules();
    let mut c = e.cmd(&bin_debug("ddalgak-gui"));
    c.args([
        "--test-host",
        "--test-max-seconds",
        "120",
        "--endpoint",
        &e.endpoint,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let mut host = Host(c.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(20);
    let st = loop {
        let (_, d) = e.run(&["status", "--endpoint", &e.endpoint]);
        if d["data"]["app"]["running"] == true {
            break d["data"].clone();
        }
        assert!(host.0.try_wait().unwrap().is_none(), "the app exited early");
        assert!(Instant::now() < until, "the app never answered");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(st["engine"]["state"], "running", "{st:#}");
    let mut active: Vec<String> = st["rules"]["active_rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap().to_string())
        .collect();
    active.sort();
    assert_eq!(
        active,
        ["global", "in-code", "in-excel", "meeting", "not-vault"],
        "{st:#}"
    );
    let inactive: Vec<(&str, &str)> = st["rules"]["inactive"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["rule"].as_str().unwrap(), i["code"].as_str().unwrap()))
        .collect();
    assert_eq!(inactive, [("newer", "condition_unsupported")]);
    let text = st.to_string().to_lowercase();
    // The Run-key location names `\Windows\` by design; no program the user runs is named.
    for leak in ["explorer.exe", "code.exe", "program files"] {
        assert!(!text.contains(leak), "status names `{leak}`");
    }
    drop(host);
}

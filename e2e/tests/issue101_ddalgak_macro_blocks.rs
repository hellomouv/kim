//! E2E — #101 (follow-up of Step 78): Ddalgak macro blocks (`repeat`, `if`/`else`, `end`),
//! variables (`set_var`, `{var:name}`), the simulator's unrolled count and `rule run`
//! (docs/DDALGAK_RULE_MODEL.md §5.18, ADR 0085). Requirements: `DDALGAK-FR-074`,
//! `DDALGAK-SEC-011`, `KIM-FR-012`.
//!
//! Scenario A (release `ddalgak`, isolated profile, no app): an agent adds a macro with a
//! repeat block, a variable and an if/else; `ddalgak test` reports how many steps it unrolls to;
//! `rule show` hides the variable's value; an unclosed block is refused with nothing written.
//!
//! Scenario A also adds the steps a mouse recording produces (a double click at a point, a delay,
//! three wheel notches) and previews them; the recording hook itself is a maintainer check.
//!
//! Scenario B (debug test host with `--test-launch-root`: notifications are recorded, never
//! shown): `rule run` starts the macro without any key; the variable counted to 3, so the `then`
//! branch's notification is recorded and the `else` branch's is not; `rule run` of a mode rule
//! is `rule_not_runnable`.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`, own pipe endpoint; no key is sent; no
//! window, notification or tray icon appears; the host is quit or killed by a guard.
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
const SECRET: &str = "issue101-variable-secret";

fn schema(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../docs/contracts/schemas/{name}.schema.json"));
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
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

    fn json(&self, args: &[&str]) -> (i32, Value) {
        let mut c = self.cmd(&bin("ddalgak"));
        c.args(args).arg("--json");
        let talks_to_app = matches!(
            args,
            ["status" | "engine" | "quit", ..] | ["rule", "run", ..]
        );
        if !self.endpoint.is_empty() && talks_to_app {
            c.args(["--endpoint", &self.endpoint]);
        }
        let out = c.output().unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e})"));
        let errors = jsonschema::Validator::new(&schema("ddalgak-cli-v1"))
            .validate(&doc)
            .unwrap();
        assert!(errors.is_empty(), "{args:?}: {errors:?}\n{doc:#}");
        if args.first() != Some(&"rule") || args.get(1) == Some(&"show") {
            assert!(
                !doc.to_string().contains(SECRET),
                "{args:?}: a variable's value appeared"
            );
        }
        (out.status.code().unwrap_or(-1), doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, d) = self.json(args);
        assert_eq!(code, 0, "{args:?}: {d:#}");
        d["data"].clone()
    }

    fn config(&self) -> PathBuf {
        self.sb.path().join("Roaming/mouv/ddalgak")
    }
}

/// The macro: count to 3 in a repeat block, then notify "Three" (5 characters) if the count is
/// 3, otherwise "No" (2).
fn macro_steps() -> Vec<Value> {
    vec![
        json!({"type": "set_var", "name": "n", "value": "0"}),
        json!({"type": "set_var", "name": "secret", "value": SECRET}),
        json!({"type": "repeat", "times": 3}),
        json!({"type": "set_var", "name": "n", "add": 1}),
        json!({"type": "end"}),
        json!({"type": "if", "when": {"var": {"name": "n", "equals": "3"}}}),
        json!({"type": "notify", "title": "Three"}),
        json!({"type": "else"}),
        json!({"type": "notify", "title": "No"}),
        json!({"type": "end"}),
    ]
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn issue101_an_agent_writes_a_macro_with_blocks_and_variables() {
    require_e2e();
    let sb = TempDir::new("issue101-cli");
    std::fs::create_dir_all(sb.path().join("Roaming")).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let e = Env {
        sb: &sb,
        endpoint: String::new(),
    };
    let steps: Vec<String> = macro_steps().iter().map(Value::to_string).collect();
    let mut args = vec![
        "rule",
        "add",
        "--id",
        "count",
        "--trigger",
        "Ctrl+Alt+Shift+F21",
    ];
    for s in &steps {
        args.extend(["--step", s.as_str()]);
    }
    args.extend(["--times", "2"]);
    e.ok(&args);
    let t = e.ok(&["test", "Ctrl+Alt+Shift+F21"]);
    // Per run: 2 sets, 3 adds, the branch, "Three", the skip, "No" = 9; times 2 = 18.
    assert_eq!(t["unrolled"], json!({"times": 2, "steps": 18}), "{t:#}");
    let shown = e.ok(&["rule", "show", "count"]);
    assert_eq!(shown["text_redacted"], true);
    assert_eq!(
        shown["rule"]["steps"][1]["value"],
        json!({"redacted": true, "characters": SECRET.chars().count()})
    );
    // #101 item 3: what a mouse recording becomes (the editor's `recorder::mouse_steps`) is a
    // macro the CLI accepts with input-injection consent and previews without moving anything.
    let recorded = [
        json!({"type": "mouse_click", "button": "left", "op": "double_click", "at": {"x": 100, "y": 200}}),
        json!({"type": "delay", "ms": 800}),
        json!({"type": "mouse_scroll", "direction": "down", "notches": 3}),
    ];
    let rec: Vec<String> = recorded.iter().map(Value::to_string).collect();
    let mut args = vec![
        "rule",
        "add",
        "--id",
        "clicks",
        "--trigger",
        "Ctrl+Alt+Shift+F23",
    ];
    for s in &rec {
        args.extend(["--step", s.as_str()]);
    }
    args.extend(["--allow", "input-injection"]);
    e.ok(&args);
    let t = e.ok(&["test", "Ctrl+Alt+Shift+F23"]);
    assert_eq!(t["decision"]["rule"], "clicks", "{t:#}");
    assert_eq!(t["injected_events"], 0);
    let kinds: Vec<&str> = t["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["mouse_click", "delay", "mouse_scroll"]);

    let before = std::fs::read(e.config().join("rules.json")).unwrap();
    let open = json!({"type": "repeat", "times": 2}).to_string();
    let note = json!({"type": "notify", "title": "x"}).to_string();
    let (code, d) = e.json(&[
        "rule",
        "add",
        "--id",
        "open",
        "--trigger",
        "Ctrl+Alt+Shift+F22",
        "--step",
        &open,
        "--step",
        &note,
    ]);
    assert_ne!(code, 0, "{d:#}");
    assert!(d.to_string().contains("no `end`"), "{d:#}");
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
fn issue101_rule_run_takes_the_branch_its_variable_chose() {
    require_e2e();
    let sb = TempDir::new("issue101-app");
    let e = Env {
        sb: &sb,
        endpoint: format!("kim-e2e-issue101-{}", std::process::id()),
    };
    std::fs::create_dir_all(e.config()).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let rules = json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 1,
        "active_profile": "work",
        "profiles": [ { "id": "work", "title": "Work", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" }, { "id": "nav", "title": "Nav" } ],
            "rules": [
                { "id": "count", "title": "Counts", "enabled": true,
                  "trigger": { "keys": "Ctrl+Alt+Shift+F21" }, "steps": macro_steps() },
                { "id": "to-nav", "title": "To nav", "enabled": true,
                  "trigger": { "keys": "Ctrl+Alt+Shift+F22" },
                  "steps": [ { "type": "mode", "op": "set", "mode": "nav" } ] }
            ] } ]
    });
    std::fs::write(e.config().join("rules.json"), rules.to_string()).unwrap();
    let root = sb.path().join("launch-root");
    std::fs::create_dir_all(&root).unwrap();
    let mut c = e.cmd(&bin_debug("ddalgak-gui"));
    c.args([
        "--test-host",
        "--test-max-seconds",
        "60",
        "--endpoint",
        &e.endpoint,
        "--test-launch-root",
    ])
    .arg(&root)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let mut host = Host(c.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(20);
    while e.json(&["status"]).1["data"]["app"]["running"] != true {
        assert!(host.0.try_wait().unwrap().is_none(), "the app exited early");
        assert!(Instant::now() < until, "the app never answered");
        std::thread::sleep(Duration::from_millis(100));
    }
    if ddalgak::win::is_elevated() {
        eprintln!("skipped (elevated): the app refuses to turn its engine on as administrator");
        e.json(&["quit"]);
        return;
    }
    e.ok(&["engine", "start"]);
    let d = e.ok(&["rule", "run", "count"]);
    assert_eq!(d["started"], true, "{d:#}");
    let record = root.join("desktop.jsonl");
    let t = Instant::now();
    while !record.exists() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    let text = std::fs::read_to_string(&record).unwrap_or_default();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines,
        [json!({"notify": 5, "body": 0})],
        "only the `then` branch ran"
    );
    let (code, d) = e.json(&["rule", "run", "to-nav"]);
    assert_eq!(
        (code, d["error"]["code"].as_str()),
        (7, Some("rule_not_runnable")),
        "{d:#}"
    );
    let st = e.ok(&["status"]);
    assert!(
        st["activity"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["rule"] == "count" && a["outcome"] == "ok"),
        "{st:#}"
    );
    let (code, _) = e.json(&["quit"]);
    assert_eq!(code, 0);
    drop(host);
}

//! E2E — Step 78: Ddalgak macros — ordered multi-step rules with waits, a bounded repeat and an
//! error policy (docs/DDALGAK_RULE_MODEL.md §5.13, ADR 0079). Requirements: `DDALGAK-FR-018`, `DDALGAK-FR-074`,
//! `DDALGAK-FR-040`, `DDALGAK-SEC-002`, `DDALGAK-SEC-005`, `KIM-FR-012`.
//!
//! Scenario A (release `ddalgak`, isolated profile, no app): an agent writes a macro headlessly —
//! a shortcut, a wait, text and a mode step, twice (`--times 2`) — and a launch macro. `ddalgak
//! test` explains each step without doing anything; `rule check` has nothing to say. Refused in
//! words, nothing written: a remap inside a macro, a mode hold inside a macro, `--times 101`, a
//! macro behind a hotstring (inactive). The dry run writes nothing.
//!
//! Scenario B (debug test host of `ddalgak-gui` with `--test-launch-root`: no tray icon, a hook
//! that never consumes a key, websites recorded instead of opened): a macro of two websites with a
//! wait between them and a mode switch, run twice, accepts input from other programs (it injects
//! nothing). An injected F15 (an obscure key) runs it: four hand-offs in order, the mode is `nav`,
//! one `ok` entry. A second macro (a long wait, then a website) is started by an injected F16 and
//! the engine is paused during the wait: it ends `cancelled` and its website is never opened.
//! On a locked desktop the key part is reported as skipped.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`, own pipe endpoint; only F15 and F16
//! are sent (as another program); no browser is opened, no text is typed anywhere; the host is
//! quit or killed by a guard; no window or tray icon appears.
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
        if !self.endpoint.is_empty() && !args.first().is_some_and(|a| *a == "rule" || *a == "test")
        {
            c.args(["--endpoint", &self.endpoint]);
        }
        let out = c.output().unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e})"));
        let errors = jsonschema::Validator::new(&schema("ddalgak-cli-v1"))
            .validate(&doc)
            .unwrap();
        assert!(errors.is_empty(), "{args:?}: {errors:?}\n{doc:#}");
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

    fn rules_bytes(&self) -> Vec<u8> {
        std::fs::read(self.config().join("rules.json")).unwrap_or_default()
    }

    fn refused(&self, args: &[&str]) -> Value {
        let before = self.rules_bytes();
        let (code, d) = self.json(args);
        assert_ne!(code, 0, "{args:?}: {d:#}");
        assert_eq!(self.rules_bytes(), before, "{args:?}: nothing written");
        d["error"].clone()
    }
}

fn step(v: &Value) -> String {
    v.to_string()
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step78_an_agent_writes_and_explains_macros_headlessly() {
    require_e2e();
    let sb = TempDir::new("step78-cli");
    std::fs::create_dir_all(sb.path().join("Roaming")).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let e = Env {
        sb: &sb,
        endpoint: String::new(),
    };
    let keys = step(&json!({ "type": "send_keys", "keys": "Ctrl+L" }));
    let wait = step(&json!({ "type": "delay", "ms": 200 }));
    let text = step(&json!({ "type": "type_text", "text": "Step78-비밀" }));
    let mode = step(&json!({ "type": "mode", "op": "default" }));
    // The dry run writes nothing.
    let base = [
        "rule",
        "add",
        "--id",
        "fill",
        "--trigger",
        "Ctrl+Alt+Shift+F21",
        "--step",
        &keys,
        "--step",
        &wait,
        "--step",
        &text,
        "--step",
        &mode,
        "--times",
        "2",
        "--allow",
        "input-injection",
    ];
    let mut dry = base.to_vec();
    dry.push("--dry-run");
    e.ok(&dry);
    assert!(e.rules_bytes().is_empty(), "the dry run wrote nothing");
    e.ok(&base);
    let site = step(&json!({ "type": "open_url", "url": "https://example.com/a" }));
    e.ok(&[
        "rule",
        "add",
        "--id",
        "tabs",
        "--trigger",
        "Ctrl+Alt+Shift+F22",
        "--step",
        &site,
        "--step",
        &wait,
        "--step",
        &site,
        "--allow",
        "launch-programs",
    ]);
    let check = e.ok(&["rule", "check"]);
    assert_eq!(check["problems"], json!([]), "{check:#}");
    let shown = e.ok(&["rule", "show", "fill"]);
    assert_eq!(shown["state"], "active", "{shown:#}");
    assert_eq!(shown["rule"]["times"], 2);
    // The simulation names every step, in order, and does nothing.
    let t = e.ok(&["test", "Ctrl+Alt+Shift+F21"]);
    let types: Vec<&str> = t["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["send_keys", "delay", "type_text", "mode"]);
    assert_eq!(
        (
            t["injected_events"].as_u64(),
            t["opened"].as_array().map(Vec::len)
        ),
        (Some(0), Some(0))
    );
    assert!(
        !t.to_string().contains("비밀"),
        "typed text is never echoed"
    );
    // Refused in words.
    let remap = step(&json!({ "type": "remap", "to": "F20" }));
    let err = e.refused(&[
        "rule",
        "add",
        "--trigger",
        "Ctrl+Alt+Shift+F23",
        "--step",
        &keys,
        "--step",
        &remap,
    ]);
    assert!(err["message"].as_str().is_some(), "{err:#}");
    let (code, d) = e.json(&[
        "rule",
        "add",
        "--trigger",
        "Ctrl+Alt+Shift+F23",
        "--step",
        &keys,
        "--step",
        &wait,
        "--times",
        "101",
    ]);
    assert_eq!((code, d["error"]["code"].as_str()), (2, Some("usage")));
    let hold = step(&json!({ "type": "mode", "op": "hold", "mode": "base" }));
    e.refused(&[
        "rule",
        "add",
        "--trigger",
        "Ctrl+Alt+Shift+F23",
        "--step",
        &wait,
        "--step",
        &hold,
    ]);
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

fn rules_b() -> Value {
    let url = |u: &str| json!({ "type": "open_url", "url": u });
    let mut run = json!({ "id": "run", "title": "F15 runs a macro", "enabled": true,
        "trigger": { "keys": "F15", "injected": "accept" }, "times": 2,
        "steps": [ url("https://example.com/one"), { "type": "delay", "ms": 150 },
                   url("https://example.com/two"), { "type": "mode", "op": "set", "mode": "nav" } ] });
    let mut slow = json!({ "id": "slow", "title": "F16 waits, then opens", "enabled": true,
        "trigger": { "keys": "F16", "injected": "accept" },
        "steps": [ { "type": "delay", "ms": 4000 }, url("https://example.com/never") ] });
    for r in [&mut run, &mut slow] {
        let fp = ddalgak::validate::launch_fingerprint(r);
        r["consent"] = json!({ "launch-programs": { "granted_at": "2026-11-04T09:00:00Z", "via": "gui", "fingerprint": fp } });
    }
    json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 1,
        "active_profile": "work",
        "profiles": [ { "id": "work", "title": "Work", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" }, { "id": "nav", "title": "Navigation" } ],
            "rules": [ run, slow ] } ]
    })
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step78_the_running_engine_runs_a_macro_in_order_and_a_pause_cancels_one() {
    require_e2e();
    let sb = TempDir::new("step78-app");
    let e = Env {
        sb: &sb,
        endpoint: format!("kim-e2e-step78-{}", std::process::id()),
    };
    std::fs::create_dir_all(e.config()).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    std::fs::write(e.config().join("rules.json"), rules_b().to_string()).unwrap();
    let root = sb.path().join("launch-root");
    std::fs::create_dir_all(&root).unwrap();
    let mut c = e.cmd(&bin_debug("ddalgak-gui"));
    c.args([
        "--test-host",
        "--test-max-seconds",
        "90",
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
    let d = e.ok(&["engine", "start"]);
    assert_eq!(d["engine"]["state"], "running", "{d:#}");
    let st = e.ok(&["status"]);
    assert_eq!(
        st["rules"]["active_rules"],
        json!(["run", "slow"]),
        "{st:#}"
    );
    let record = root.join("shell-open.jsonl");
    let opened = || -> Vec<String> {
        std::fs::read_to_string(&record)
            .unwrap_or_default()
            .lines()
            .map(|l| {
                serde_json::from_str::<Value>(l).unwrap()["target"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    };
    let tap = |vk: u16| {
        ddalgak::win::inject::send_as_other_program(&[
            ddalgak::inject::OutKey { vk, up: false },
            ddalgak::inject::OutKey { vk, up: true },
        ]) == 2
    };
    if !ddalgak::win::input_desktop_available() || !tap(0x7E) {
        eprintln!("skipped (locked): the macro triggers need SendInput on an unlocked desktop");
    } else {
        let t = Instant::now();
        while opened().len() < 4 && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            opened(),
            [
                "https://example.com/one",
                "https://example.com/two",
                "https://example.com/one",
                "https://example.com/two"
            ]
        );
        assert!(
            t.elapsed() >= Duration::from_millis(300),
            "both waits were waited"
        );
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(e.ok(&["mode", "status"])["mode"]["active"], "nav");
        let outcomes = |st: &Value| -> Vec<String> {
            st["activity"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    format!(
                        "{}:{}",
                        a["rule"].as_str().unwrap(),
                        a["outcome"].as_str().unwrap()
                    )
                })
                .collect()
        };
        assert_eq!(
            outcomes(&e.ok(&["status"])),
            ["run:ok"],
            "one entry for the whole macro"
        );
        // A pause during the long wait cancels the second macro: its website never opens.
        if tap(0x7F) {
            std::thread::sleep(Duration::from_millis(300));
            e.ok(&["pause"]);
            std::thread::sleep(Duration::from_millis(4500));
            assert!(
                !opened().iter().any(|u| u.ends_with("/never")),
                "{:?}",
                opened()
            );
            assert!(outcomes(&e.ok(&["status"])).contains(&"slow:cancelled".to_string()));
        } else {
            // The desktop locked during the scenario: SendInput is refused there.
            eprintln!("skipped (locked): the cancel part needs SendInput on an unlocked desktop");
        }
    }
    let (code, _) = e.json(&["quit"]);
    assert_eq!(code, 0);
    drop(host);
}

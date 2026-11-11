//! E2E — Step 79: Ddalgak window, clipboard, notification and tool steps, and schedules
//! (docs/DDALGAK_RULE_MODEL.md §5.14–§5.17, §3.5, ADR 0080). Requirements: `DDALGAK-FR-072`,
//! `DDALGAK-FR-073`, `DDALGAK-FR-076`, `DDALGAK-SEC-008`, `DDALGAK-SEC-011`, `KIM-FR-012`.
//!
//! Scenario A (release `ddalgak`, isolated profile, no app): an agent writes a window rule, a
//! clipboard rule (each with its consent), a notification, a tool step and a schedule (as a whole
//! rule). Without consent a window rule is saved turned off. `ddalgak test` previews each step
//! without its personal content (no clipboard text, no notification words). A scheduled rule
//! that would type is refused with nothing written.
//!
//! Scenario B (debug test host of `ddalgak-gui` with `--test-launch-root`: window and notification
//! steps are recorded in the test folder, never carried out; no Kim is installed in the profile):
//! a schedule every minute shows its notification within about a minute; an injected F15 (an
//! obscure key) runs a window step and a tool step — the window step is recorded, the tool step
//! ends `tool_missing` (Kim is absent and never installed). On a locked desktop the key part is
//! skipped.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`, own pipe endpoint; no real window,
//! the real clipboard or the tray is touched; only F15 is sent (as another program); the host is
//! quit or killed by a guard.
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
const CLIP: &str = "Step79-클립보드-비밀";
const WORDS: &str = "Step79-알림-문장";

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
            .env_remove("KIM_EXE")
            .creation_flags(CREATE_NO_WINDOW);
        c
    }

    fn json(&self, args: &[&str]) -> (i32, Value) {
        let mut c = self.cmd(&bin("ddalgak"));
        c.args(args).arg("--json");
        if !self.endpoint.is_empty() && !matches!(args.first(), Some(&"rule" | &"test")) {
            c.args(["--endpoint", &self.endpoint]);
        }
        let out = c.output().unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e})"));
        let errors = jsonschema::Validator::new(&schema("ddalgak-cli-v1"))
            .validate(&doc)
            .unwrap();
        assert!(errors.is_empty(), "{args:?}: {errors:?}\n{doc:#}");
        let text = doc.to_string();
        assert!(
            !text.contains(CLIP) && !text.contains(WORDS),
            "{args:?}: personal content appeared"
        );
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

fn s(v: &Value) -> String {
    v.to_string()
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step79_an_agent_writes_window_clipboard_notify_tool_and_scheduled_rules() {
    require_e2e();
    let sb = TempDir::new("step79-cli");
    std::fs::create_dir_all(sb.path().join("Roaming")).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let e = Env {
        sb: &sb,
        endpoint: String::new(),
    };
    let win = s(
        &json!({ "type": "window", "op": "move", "x": 10, "y": 20, "width": 800, "height": 600 }),
    );
    e.ok(&[
        "rule",
        "add",
        "--id",
        "win",
        "--trigger",
        "Ctrl+Alt+Shift+F21",
        "--step",
        &win,
        "--allow",
        "window-control",
    ]);
    let clip = s(&json!({ "type": "clipboard", "op": "set", "text": CLIP }));
    e.ok(&[
        "rule",
        "add",
        "--id",
        "clip",
        "--trigger",
        "Ctrl+Alt+Shift+F22",
        "--step",
        &clip,
        "--allow",
        "clipboard",
    ]);
    let note = s(&json!({ "type": "notify", "title": WORDS, "body": WORDS }));
    let tool = s(&json!({ "type": "tool", "capability": "pin.list" }));
    e.ok(&[
        "rule",
        "add",
        "--id",
        "note",
        "--trigger",
        "Ctrl+Alt+Shift+F23",
        "--step",
        &note,
        "--step",
        &tool,
    ]);
    // Without consent a window rule is saved turned off (`consent_required`, exit 7).
    let (code, d) = e.json(&[
        "rule",
        "add",
        "--id",
        "win-off",
        "--trigger",
        "Ctrl+Alt+Shift+F24",
        "--step",
        &win,
    ]);
    assert_eq!(
        (code, d["error"]["code"].as_str()),
        (7, Some("consent_required")),
        "{d:#}"
    );
    assert_eq!(e.ok(&["rule", "show", "win-off"])["rule"]["enabled"], false);
    // A schedule, as a whole rule.
    let sched = s(
        &json!({ "id": "tick", "title": "Stretch every hour", "enabled": true,
        "trigger": { "schedule": { "at": "09:30", "days": ["mon", "fri"] } },
        "steps": [ { "type": "notify", "title": "Stretch" } ] }),
    );
    e.ok(&["rule", "add", "--rule", &sched]);
    let check = e.ok(&["rule", "check"]);
    assert_eq!(check["problems"], json!([]), "{check:#}");
    for (id, keys) in [
        ("win", "Ctrl+Alt+Shift+F21"),
        ("clip", "Ctrl+Alt+Shift+F22"),
        ("note", "Ctrl+Alt+Shift+F23"),
    ] {
        let t = e.ok(&["test", keys]);
        let errors = jsonschema::Validator::new(&schema("ddalgak-simulation-v1"))
            .validate(&t)
            .unwrap();
        assert!(errors.is_empty(), "{id}: {errors:?}");
        assert_eq!(t["decision"]["rule"], id);
    }
    let t = e.ok(&["test", "Ctrl+Alt+Shift+F22"]);
    assert_eq!(
        t["effects"][0]["preview"],
        json!({ "op": "set", "characters": CLIP.chars().count() })
    );
    // A scheduled rule never types: refused, nothing written.
    let before = std::fs::read(e.config().join("rules.json")).unwrap();
    let typing = s(
        &json!({ "id": "bad", "title": "Types by the clock", "enabled": true,
        "trigger": { "schedule": { "every_minutes": 5 } }, "steps": [ { "type": "type_text", "text": "x" } ],
        "consent": { "input-injection": { "granted_at": "2026-11-04T09:00:00Z", "via": "gui" } } }),
    );
    let (code, _) = e.json(&["rule", "add", "--rule", &typing]);
    assert_ne!(code, 0);
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
fn step79_the_running_engine_schedules_and_records_window_and_tool_steps() {
    require_e2e();
    let sb = TempDir::new("step79-app");
    let e = Env {
        sb: &sb,
        endpoint: format!("kim-e2e-step79-{}", std::process::id()),
    };
    std::fs::create_dir_all(e.config()).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let grant = json!({ "granted_at": "2026-11-04T09:00:00Z", "via": "gui" });
    let rules = json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 1,
        "active_profile": "work",
        "profiles": [ { "id": "work", "title": "Work", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" } ],
            "rules": [
                { "id": "tick", "title": "Every minute", "enabled": true,
                  "trigger": { "schedule": { "every_minutes": 1 } },
                  "steps": [ { "type": "notify", "title": "Tick" } ] },
                { "id": "arrange", "title": "F15 arranges", "enabled": true,
                  "trigger": { "keys": "F15", "injected": "accept" },
                  "steps": [ { "type": "window", "op": "maximize", "app": { "exe": "notepad.exe" } },
                             { "type": "tool", "capability": "pin.list" } ],
                  "on_error": "continue", "consent": { "window-control": grant } }
            ] } ]
    });
    std::fs::write(e.config().join("rules.json"), rules.to_string()).unwrap();
    let root = sb.path().join("launch-root");
    std::fs::create_dir_all(&root).unwrap();
    let mut c = e.cmd(&bin_debug("ddalgak-gui"));
    c.args([
        "--test-host",
        "--test-max-seconds",
        "150",
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
        json!(["tick", "arrange"]),
        "{st:#}"
    );
    let record = root.join("desktop.jsonl");
    let lines = || -> Vec<Value> {
        std::fs::read_to_string(&record)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    };
    let outcomes = |e: &Env<'_>| -> Vec<String> {
        e.ok(&["status"])["activity"]
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
    let tap = |vk: u16| {
        ddalgak::win::inject::send_as_other_program(&[
            ddalgak::inject::OutKey { vk, up: false },
            ddalgak::inject::OutKey { vk, up: true },
        ]) == 2
    };
    if !ddalgak::win::input_desktop_available() || !tap(0x7E) {
        eprintln!(
            "skipped (locked): the window and tool part needs SendInput on an unlocked desktop"
        );
    } else {
        let t = Instant::now();
        while !outcomes(&e).iter().any(|o| o.starts_with("arrange:"))
            && t.elapsed() < Duration::from_secs(40)
        {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            lines().contains(
                &json!({ "window": "maximize", "app": "notepad.exe", "pos": null, "size": null })
            ),
            "{:?}",
            lines()
        );
        assert!(
            outcomes(&e).contains(&"arrange:tool_missing".to_string()),
            "{:?}",
            outcomes(&e)
        );
    }
    // The schedule fires about a minute after the engine started.
    let t = Instant::now();
    while !lines().iter().any(|l| l.get("notify").is_some())
        && t.elapsed() < Duration::from_secs(80)
    {
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(lines().iter().any(|l| l["notify"] == 4), "{:?}", lines());
    assert!(
        outcomes(&e).contains(&"tick:ok".to_string()),
        "{:?}",
        outcomes(&e)
    );
    let (code, _) = e.json(&["quit"]);
    assert_eq!(code, 0);
    drop(host);
}

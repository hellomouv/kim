//! E2E — #102 (follow-up of Step 79): Ddalgak `run_command` steps and the `run-commands`
//! permission (docs/DDALGAK_RULE_MODEL.md §5.14–§5.17, §6.1–§6.2, ADR 0083). Requirements:
//! `DDALGAK-SEC-001`, `DDALGAK-SEC-011`, `KIM-FR-012`.
//!
//! Scenario A (release `ddalgak`, isolated profile, no app): an agent adds a command rule without
//! consent (saved turned off), then with `--allow run-commands` (on, the grant bound to a
//! fingerprint); `ddalgak test` previews the program's file name, the argument count and the
//! timeout, never the arguments, and runs nothing; a relative program is refused with nothing
//! written.
//!
//! Scenario B (debug test host with `--test-launch-root`: commands are recorded, never run): an
//! injected F15 (an obscure key) runs the command step; the record has the program's file name
//! and the argument count, and the activity has neither. On a locked desktop it is skipped.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`, own pipe endpoint; no command runs;
//! only F15 is sent (as another program); the host is quit or killed by a guard.
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
const SECRET: &str = "--token=issue102-secret";

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
        if args.first() != Some(&"rule") {
            assert!(
                !doc.to_string().contains(SECRET),
                "{args:?}: an argument appeared"
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

fn program() -> String {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{sys}\System32\where.exe")
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn issue102_an_agent_adds_a_command_rule_that_needs_its_own_consent() {
    require_e2e();
    let sb = TempDir::new("issue102-cli");
    std::fs::create_dir_all(sb.path().join("Roaming")).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let e = Env {
        sb: &sb,
        endpoint: String::new(),
    };
    let step = json!({ "type": "run_command", "program": program(), "args": ["/q", SECRET],
        "timeout_ms": 5000 })
    .to_string();
    let (code, d) = e.json(&[
        "rule",
        "add",
        "--id",
        "off",
        "--trigger",
        "Ctrl+Alt+Shift+F21",
        "--step",
        &step,
    ]);
    // Saved turned off, exit 7 `consent_required` (CLI_CONTRACT).
    assert_eq!(code, 7, "{d:#}");
    assert_eq!(d["error"]["code"], "consent_required", "{d:#}");
    e.ok(&[
        "rule",
        "add",
        "--id",
        "cmd",
        "--trigger",
        "Ctrl+Alt+Shift+F22",
        "--step",
        &step,
        "--allow",
        "run-commands",
    ]);
    let doc: Value =
        serde_json::from_slice(&std::fs::read(e.config().join("rules.json")).unwrap()).unwrap();
    let rule = doc["profiles"][0]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "cmd")
        .unwrap();
    assert_eq!(rule["enabled"], true);
    assert!(
        rule["consent"]["run-commands"]["fingerprint"]
            .as_str()
            .is_some_and(|f| f.starts_with("sha256:")),
        "{rule:#}"
    );
    let errors = jsonschema::Validator::new(&schema("ddalgak-rules-v1"))
        .validate(&doc)
        .unwrap();
    assert!(errors.is_empty(), "{errors:?}");

    let t = e.ok(&["test", "Ctrl+Alt+Shift+F22"]);
    let text = t.to_string();
    assert!(
        text.contains("where.exe") && !text.contains(SECRET),
        "{t:#}"
    );
    assert!(text.contains("\"arguments\":2"), "{t:#}");

    let before = std::fs::read(e.config().join("rules.json")).unwrap();
    let bad = json!({ "type": "run_command", "program": "where.exe" }).to_string();
    let (code, d) = e.json(&[
        "rule",
        "add",
        "--id",
        "rel",
        "--trigger",
        "Ctrl+Alt+Shift+F23",
        "--step",
        &bad,
        "--allow",
        "run-commands",
    ]);
    assert_ne!(code, 0, "{d:#}");
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
fn issue102_the_running_engine_records_a_command_without_its_arguments() {
    require_e2e();
    let sb = TempDir::new("issue102-app");
    let e = Env {
        sb: &sb,
        endpoint: format!("kim-e2e-issue102-{}", std::process::id()),
    };
    std::fs::create_dir_all(e.config()).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let mut rule = json!({ "id": "cmd", "title": "F15 runs a command", "enabled": true,
        "trigger": { "keys": "F15", "injected": "accept" },
        "steps": [ { "type": "run_command", "program": program(), "args": ["/q", SECRET] } ] });
    let fp =
        ddalgak::validate::fingerprint_for(ddalgak::model::Permission::RunCommands, &rule).unwrap();
    rule["consent"] = json!({ "run-commands": { "granted_at": "2026-11-12T09:00:00Z",
        "via": "gui", "fingerprint": fp } });
    let rules = json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 1,
        "active_profile": "work",
        "profiles": [ { "id": "work", "title": "Work", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" } ], "rules": [ rule ] } ]
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
    let d = e.ok(&["engine", "start"]);
    assert_eq!(d["engine"]["state"], "running", "{d:#}");
    assert_eq!(e.ok(&["status"])["rules"]["active_rules"], json!(["cmd"]));
    let tap = |vk: u16| {
        ddalgak::win::inject::send_as_other_program(&[
            ddalgak::inject::OutKey { vk, up: false },
            ddalgak::inject::OutKey { vk, up: true },
        ]) == 2
    };
    if !ddalgak::win::input_desktop_available() || !tap(0x7E) {
        eprintln!("skipped (locked): the command part needs SendInput on an unlocked desktop");
    } else {
        let record = root.join("commands.jsonl");
        let t = Instant::now();
        while !record.exists() && t.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let text = std::fs::read_to_string(&record).unwrap_or_default();
        let line: Value = serde_json::from_str(text.lines().next().unwrap_or("null")).unwrap();
        assert_eq!(
            line,
            json!({ "program": "where.exe", "arguments": 2, "timeout_ms": 60000 })
        );
        assert!(!text.contains(SECRET));
        let t = Instant::now();
        loop {
            let act = e.ok(&["status"])["activity"].clone();
            if act
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x["rule"] == "cmd"))
            {
                assert!(act.to_string().contains("\"ok\""), "{act:#}");
                assert!(!act.to_string().contains("where"), "{act:#}");
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(10), "{act:#}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let (code, _) = e.json(&["quit"]);
    assert_eq!(code, 0);
    drop(host);
}

//! E2E — #100 (follow-up of Step 77): Ddalgak `when.window_class` and `when.time` conditions and
//! opt-in per-app profiles (docs/DDALGAK_RULE_MODEL.md §7.2, §10.2, ADR 0087). Requirements:
//! `DDALGAK-FR-072`, `DDALGAK-SEC-011`, `KIM-FR-012`.
//!
//! Scenario A (release `ddalgak`, isolated profile, no app): an agent adds a rule for a window
//! class during working hours; `ddalgak explain --context` shows it wins with the class at 10:00
//! on a Thursday, loses with another class, and loses on a Saturday; a malformed time is refused
//! with nothing written.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`; nothing is pressed, opened or shown.
#![cfg(windows)]

#[path = "../../xtask/src/jsonschema.rs"]
mod jsonschema;

use kim_e2e::{TempDir, bin, require_e2e};
use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn schema(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../docs/contracts/schemas/{name}.schema.json"));
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn run(sb: &TempDir, args: &[&str]) -> (i32, Value) {
    let out = Command::new(bin("ddalgak"))
        .args(args)
        .arg("--json")
        .env("APPDATA", sb.path().join("Roaming"))
        .env("LOCALAPPDATA", sb.path().join("Local"))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let errors = jsonschema::Validator::new(&schema("ddalgak-cli-v1"))
        .validate(&doc)
        .unwrap();
    assert!(errors.is_empty(), "{args:?}: {errors:?}\n{doc:#}");
    (out.status.code().unwrap_or(-1), doc)
}

fn config(sb: &TempDir) -> PathBuf {
    sb.path().join("Roaming/mouv/ddalgak")
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn issue100_class_and_time_conditions_decide_in_explain() {
    require_e2e();
    let sb = TempDir::new("issue100-cli");
    std::fs::create_dir_all(sb.path().join("Roaming")).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let note = json!({"type": "notify", "title": "Work"}).to_string();
    let (code, d) = run(
        &sb,
        &[
            "rule",
            "add",
            "--id",
            "work",
            "--trigger",
            "Ctrl+Alt+Shift+F21",
            "--step",
            &note,
        ],
    );
    assert_eq!(code, 0, "{d:#}");
    // `rule edit --patch` sets the condition the way an agent would.
    let when = json!({"when": {"window_class": {"equals": "Chrome_WidgetWin_1"},
        "time": {"from": "09:00", "to": "17:30", "days": ["mon", "tue", "wed", "thu", "fri"]}}})
    .to_string();
    let (code, d) = run(&sb, &["rule", "edit", "work", "--patch", &when]);
    assert_eq!(code, 0, "{d:#}");
    let explain = |class: &str, at: &str| {
        let ctx = json!({"app": {"exe": "chrome.exe", "window_class": class}, "local_time": at})
            .to_string();
        let (code, d) = run(&sb, &["explain", "Ctrl+Alt+Shift+F21", "--context", &ctx]);
        assert_eq!(code, 0, "{d:#}");
        let errors = jsonschema::Validator::new(&schema("ddalgak-simulation-v1"))
            .validate(&d["data"])
            .unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        d["data"]["decision"]["outcome"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // 2026-11-12 is a Thursday, 2026-11-14 a Saturday.
    assert_eq!(explain("Chrome_WidgetWin_1", "2026-11-12T10:00"), "fired");
    assert_eq!(explain("Notepad", "2026-11-12T10:00"), "no_match");
    assert_eq!(
        explain("Chrome_WidgetWin_1", "2026-11-14T10:00"),
        "no_match"
    );
    assert_eq!(
        explain("Chrome_WidgetWin_1", "2026-11-12T17:30"),
        "no_match"
    );
    let before = std::fs::read(config(&sb).join("rules.json")).unwrap();
    let bad = json!({"when": {"time": {"from": "9am", "to": "17:00"}}}).to_string();
    let (code, d) = run(&sb, &["rule", "edit", "work", "--patch", &bad]);
    assert_ne!(code, 0, "{d:#}");
    assert_eq!(
        std::fs::read(config(&sb).join("rules.json")).unwrap(),
        before
    );
}

struct Host(std::process::Child);

impl Drop for Host {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// Scenario B (debug test host, no tray, no window): with `settings.app_profiles` enabled for the
/// program that is in front right now, the running engine loads that profile in memory
/// (`status.profile.by_app`, `saved` stays the file's) without rewriting rules.json; turning the
/// setting off gives the saved profile back. Skipped when nothing readable is in front (locked,
/// elevated) or when the person at the computer switches windows meanwhile.
#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn issue100_the_program_in_front_chooses_a_profile_in_memory() {
    use ddalgak::context::Foreground;
    require_e2e();
    let front = || ddalgak::win::context::foreground().app();
    let Some(app) = front() else {
        eprintln!("skipped: no readable program is in front (locked or elevated)");
        return;
    };
    let exe = app.rsplit('\\').next().unwrap().to_string();
    let sb = TempDir::new("issue100-app");
    let cfg = config(&sb);
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let doc = |enabled: bool| {
        json!({
            "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 1,
            "active_profile": "default",
            "settings": {"app_profiles": {"enabled": enabled, "map": [
                {"apps": [{"exe": exe}], "profile": "focus"}]}},
            "profiles": [
                {"id": "default", "title": "Default", "default_mode": "base",
                 "modes": [{"id": "base", "title": "Base"}], "rules": []},
                {"id": "focus", "title": "Focus", "default_mode": "base",
                 "modes": [{"id": "base", "title": "Base"}], "rules": []}
            ]
        })
        .to_string()
    };
    std::fs::write(cfg.join("rules.json"), doc(false)).unwrap();
    let ep = format!("kim-e2e-issue100-{}", std::process::id());
    let status = || {
        let (_, d) = run(&sb, &["status", "--endpoint", &ep]);
        d["data"].clone()
    };
    let mut c = Command::new(kim_e2e::bin_debug("ddalgak-gui"));
    c.args(["--test-host", "--test-max-seconds", "60", "--endpoint", &ep])
        .env("APPDATA", sb.path().join("Roaming"))
        .env("LOCALAPPDATA", sb.path().join("Local"))
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let host = Host(c.spawn().unwrap());
    let wait = |what: &str, cond: &dyn Fn(&Value) -> bool| -> Option<Value> {
        let t = std::time::Instant::now();
        loop {
            let s = status();
            if cond(&s) {
                return Some(s);
            }
            if t.elapsed() > std::time::Duration::from_secs(15) {
                eprintln!("timed out waiting for {what}: {s:#}");
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    wait("the app", &|s| s["app"]["running"] == true).expect("the app answers");
    if ddalgak::win::is_elevated() {
        eprintln!("skipped (elevated)");
        run(&sb, &["quit", "--endpoint", &ep]);
        return;
    }
    let (code, _) = run(&sb, &["engine", "start", "--endpoint", &ep]);
    assert_eq!(code, 0);
    assert_eq!(status()["profile"]["by_app"], false, "off unless enabled");
    let enabled = doc(true);
    std::fs::write(cfg.join("rules.json"), &enabled).unwrap();
    let got = wait("the per-app profile", &|s| s["profile"]["id"] == "focus");
    if front().as_deref() != Some(app.as_str()) {
        eprintln!("skipped: the program in front changed during the test");
        run(&sb, &["quit", "--endpoint", &ep]);
        return;
    }
    let s = got.expect("the profile of the program in front is loaded");
    assert_eq!(
        (
            &s["profile"]["saved"],
            &s["profile"]["by_app"],
            &s["profile"]["app_profiles"]
        ),
        (&json!("default"), &json!(true), &json!(true)),
        "{s:#}"
    );
    assert_eq!(
        std::fs::read_to_string(cfg.join("rules.json")).unwrap(),
        enabled,
        "rules.json is not rewritten"
    );
    std::fs::write(cfg.join("rules.json"), doc(false)).unwrap();
    let s = wait("the saved profile back", &|s| {
        s["profile"]["id"] == "default"
    })
    .expect("back");
    assert_eq!(s["profile"]["by_app"], false);
    let (code, _) = run(&sb, &["quit", "--endpoint", &ep]);
    assert_eq!(code, 0);
    drop(host);
}

//! E2E — Step 60: Noeul's capture history and pin backup (docs/adr/0074-noeul-history-and-pin-backup.md).
//! Requirements: NOEUL-FR-050, NOEUL-FR-051, NOEUL-FR-066, NOEUL-SEC-001, NOEUL-SEC-005.
//!
//! Scenario A (release `noeul`, isolated profile, nothing running): an agent finds the history and
//! backup settings off by default in `config show`, the new `capture.private` key (`P`) in
//! `hotkeys list`, `pin backup status` and `history clear --yes` answering without a running
//! Noeul, and no file in the data folder.
//!
//! Scenario B (debug `noeul` with the test hook `NOEUL_DEV_SYNTHETIC_DESKTOP=1`, a picture far
//! off-screen, nothing read from the screen): the user turns on the disk history and the pin
//! backup, captures and pins two regions, quits; at the next start nothing is restored by
//! itself, the history is back, `pin restore` brings both pins back once and a second restore
//! duplicates nothing; then the user removes every trace with `history clear` and `pin backup
//! clear` after quitting.
//!
//! The machine is in use while e2e runs: no tray icon, no visible window, no hotkey registered,
//! the clipboard untouched; every process started here is ended.
#![allow(unsafe_code)]

#[path = "../../xtask/src/jsonschema.rs"]
mod jsonschema;

use kim_e2e::{TempDir, bin};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn schema(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs/contracts/schemas")
        .join(format!("{name}.schema.json"));
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn valid(name: &str, v: &Value) {
    let (file, def) = name
        .split_once('#')
        .map_or((name, None), |(f, d)| (f, Some(d)));
    let mut s = schema(file);
    if let Some(d) = def {
        s = json!({"$ref": format!("#/$defs/{d}"), "$defs": s["$defs"].clone()});
    }
    let errors = jsonschema::Validator::new(&s).validate(v).unwrap();
    assert!(errors.is_empty(), "violates {name}: {errors:?}\n{v:#}");
}

struct Noeul {
    sb: TempDir,
    exe: PathBuf,
    endpoint: String,
    synthetic: bool,
}

impl Noeul {
    fn new(tag: &str, exe: PathBuf, synthetic: bool) -> Noeul {
        let sb = TempDir::new(tag);
        let cfg = sb.path().join("Roaming").join("mouv").join("noeul");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(sb.path().join("Local")).unwrap();
        std::fs::create_dir_all(sb.path().join("Temp")).unwrap();
        std::fs::create_dir_all(sb.path().join("clipboard")).unwrap();
        std::fs::write(
            cfg.join("hotkeys.json"),
            serde_json::to_vec(&json!({"schema": "kim.hotkeys", "schema_version": 1,
                "tool": "noeul", "overrides": [
                    {"action": "capture.region", "disabled": true},
                    {"action": "paste.float", "disabled": true}]}))
            .unwrap(),
        )
        .unwrap();
        Noeul {
            endpoint: format!("noeul-e2e-{tag}-{}", std::process::id()),
            sb,
            exe,
            synthetic,
        }
    }

    fn data(&self) -> PathBuf {
        self.sb.path().join("Local").join("mouv").join("noeul")
    }

    fn raw(&self, args: &[&str]) -> std::process::Output {
        let p = self.sb.path();
        let mut c = Command::new(&self.exe);
        c.args(args)
            .args(["--json", "--endpoint", &self.endpoint])
            .env("APPDATA", p.join("Roaming"))
            .env("LOCALAPPDATA", p.join("Local"))
            .env("TEMP", p.join("Temp"))
            .env("TMP", p.join("Temp"))
            .env("KIM_NO_INPUT", "1")
            .env("NOEUL_DEV_CLIPBOARD_DIR", p.join("clipboard"))
            .env_remove("NOEUL_DEV_CLIPBOARD")
            .env_remove("KIM_NO_HOTKEYS")
            .stdin(Stdio::null());
        if self.synthetic {
            c.env("NOEUL_DEV_SYNTHETIC_DESKTOP", "1");
        } else {
            c.env_remove("NOEUL_DEV_SYNTHETIC_DESKTOP");
        }
        c.output().unwrap()
    }

    fn json(&self, args: &[&str]) -> (i32, Value) {
        let out = self.raw(args);
        let doc: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e})"));
        valid("cli-envelope-v1", &doc);
        (out.status.code().unwrap_or(-1), doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.json(args);
        assert_eq!(code, 0, "{args:?}: {doc:#}");
        doc["data"].clone()
    }

    fn start(&self) {
        let (code, s) = self.json(&["start", "--no-tray"]);
        assert_eq!(code, 0, "{s:#}");
    }

    fn files(&self, sub: &str) -> usize {
        std::fs::read_dir(self.data().join(sub)).map_or(0, |d| {
            d.flatten()
                .filter(|f| f.file_name().to_string_lossy().ends_with(".png"))
                .count()
        })
    }
}

impl Drop for Noeul {
    fn drop(&mut self) {
        let out = self.raw(&["status"]);
        let pid = serde_json::from_slice::<Value>(&out.stdout)
            .ok()
            .and_then(|v| v["data"]["instance"]["pid"].as_u64());
        let _ = self.raw(&["quit", "--yes"]);
        std::thread::sleep(Duration::from_millis(500));
        if let Some(pid) = pid {
            let _ = Command::new("taskkill")
                .args([
                    "/F",
                    "/FI",
                    &format!("PID eq {pid}"),
                    "/FI",
                    "IMAGENAME eq noeul.exe",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_history_and_backup_are_off_by_default_and_clearable_without_noeul() {
    let n = Noeul::new("s60a", bin("noeul"), false);
    let show = n.ok(&["config", "show"]);
    assert_eq!(
        show["settings"]["history"],
        json!({"count": 20, "memory_mb": 256, "disk": "off", "keep_days": 7, "keep_cancelled": "off"})
    );
    assert_eq!(show["settings"]["pins"]["backup"], "off");
    let (_, list) = n.json(&["hotkeys", "list"]);
    let private = list["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "capture.private")
        .expect("capture.private is listed")
        .clone();
    assert_eq!(private["bindings"][0]["keys"], "P");
    let b = n.ok(&["pin", "backup", "status"]);
    valid("noeul-ipc-v1#backup_status_result", &b);
    assert_eq!(
        (b["enabled"].as_bool(), b["pins"].as_u64()),
        (Some(false), Some(0))
    );
    let (code, e) = n.json(&["history", "clear"]);
    assert_eq!(
        (code, e["error"]["code"].as_str()),
        (7, Some("confirmation_required"))
    );
    let c = n.ok(&["history", "clear", "--yes"]);
    assert_eq!(c["disk_cleared"], 0);
    assert!(!n.data().join("history").exists() && !n.data().join("pins").exists());
}

#[cfg(windows)]
#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn b_the_user_keeps_history_and_pins_across_a_restart_then_removes_every_trace() {
    let n = Noeul::new("s60b", kim_e2e::bin_debug("noeul"), true);
    n.ok(&["config", "set", "history.disk", "on"]);
    n.ok(&["config", "set", "pins.backup", "on"]);
    n.start();
    for x in [-19_980, -19_960] {
        let r = n.ok(&["capture", "--region", &format!("{x},-19980,8,6"), "--pin"]);
        assert!(r["pin"]["id"].is_string(), "{r:#}");
    }
    let until = Instant::now() + Duration::from_secs(15);
    while !n.data().join("pins").join("backup.json").is_file() || n.files("history") < 2 {
        assert!(Instant::now() < until, "nothing was written");
        std::thread::sleep(Duration::from_millis(100));
    }
    n.ok(&["quit", "--yes"]);
    n.start();
    assert_eq!(
        n.ok(&["status"])["pins"]["open"],
        0,
        "nothing restored by itself"
    );
    let l = n.ok(&["history", "list"]);
    valid("noeul-ipc-v1#history_list_result", &l);
    assert_eq!(l["count"], 2);
    let r = n.ok(&["pin", "restore"]);
    valid("noeul-ipc-v1#backup_restore_result", &r);
    assert_eq!(r["restored"].as_array().unwrap().len(), 2, "{r:#}");
    let r = n.ok(&["pin", "restore"]);
    assert_eq!(r["already_open"], 2);
    assert_eq!(n.ok(&["status"])["pins"]["open"], 2, "no duplicates");
    n.ok(&["quit", "--yes"]);
    let c = n.ok(&["history", "clear", "--yes"]);
    assert_eq!(c["disk_cleared"], 2);
    let c = n.ok(&["pin", "backup", "clear", "--yes"]);
    assert_eq!(c["removed"], 2);
    assert_eq!(n.files("history"), 0);
    assert!(!n.data().join("pins").exists());
    assert!(
        std::fs::read_dir(n.sb.path().join("Temp"))
            .unwrap()
            .next()
            .is_none(),
        "no temporary files"
    );
}

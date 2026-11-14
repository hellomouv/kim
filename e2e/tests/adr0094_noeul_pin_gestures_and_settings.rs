//! E2E — ADR 0093 / ADR 0094: Noeul's pin gestures and its settings window, headless.
//! Requirements: NOEUL-FR-026, NOEUL-FR-101, NOEUL-NFR-012.
//!
//! The debug `noeul` with the synthetic-desktop test hook (nothing on screen, no hotkey, no input
//! synthesized), on a private endpoint: `[pins] double_click` is `close` by default, `config set`
//! changes it to `none` and back (validated against the schemas), a wrong value is refused with
//! `usage`, and the running instance answers `status` with the setting. `noeul-settings` (the
//! settings window's own program) answers `--help` and `--version` without opening a window, and
//! refuses an unknown option with exit 2. The instance is quit at the end.
#![cfg(windows)]

#[path = "../../xtask/src/jsonschema.rs"]
mod jsonschema;

use kim_e2e::{TempDir, bin_debug, require_e2e};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn valid(name: &str, v: &Value) {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/contracts/schemas")
        .join(format!("{name}.schema.json"));
    let s: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    let errors = jsonschema::Validator::new(&s).validate(v).unwrap();
    assert!(errors.is_empty(), "violates {name}: {errors:?}\n{v:#}");
}

struct Noeul {
    sb: TempDir,
    exe: PathBuf,
    endpoint: String,
}

impl Noeul {
    fn new() -> Noeul {
        let sb = TempDir::new("adr0094");
        let cfg = sb.path().join("Roaming/mouv/noeul");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(sb.path().join("Local")).unwrap();
        std::fs::create_dir_all(sb.path().join("Temp")).unwrap();
        std::fs::write(
            cfg.join("hotkeys.json"),
            serde_json::to_vec(&json!({"schema": "kim.hotkeys", "schema_version": 1,
                "tool": "noeul", "overrides": [
                    {"action": "capture.region", "disabled": true},
                    {"action": "paste.float", "disabled": true}]}))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            sb.path().join("layout.json"),
            serde_json::to_vec(
                &json!({"monitors": [{"x": 0, "y": 0, "width": 320, "height": 200,
                "primary": true}], "pointer": {"x": 5, "y": 5}}),
            )
            .unwrap(),
        )
        .unwrap();
        Noeul {
            endpoint: format!("noeul-e2e-adr0094-{}", std::process::id()),
            sb,
            exe: bin_debug("noeul"),
        }
    }

    fn json(&self, args: &[&str]) -> (i32, Value) {
        let p = self.sb.path();
        let out = Command::new(&self.exe)
            .args(args)
            .args(["--endpoint", &self.endpoint, "--json"])
            .env("APPDATA", p.join("Roaming"))
            .env("LOCALAPPDATA", p.join("Local"))
            .env("TEMP", p.join("Temp"))
            .env("TMP", p.join("Temp"))
            .env("KIM_NO_INPUT", "1")
            .env("NOEUL_DEV_SYNTHETIC_DESKTOP", "1")
            .env("NOEUL_DEV_SYNTHETIC_LAYOUT", p.join("layout.json"))
            .env_remove("KIM_NO_HOTKEYS")
            .stdin(Stdio::null())
            .output()
            .unwrap();
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
}

impl Drop for Noeul {
    fn drop(&mut self) {
        let _ = self.json(&["quit", "--yes"]);
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn adr0094_double_click_on_a_pin_is_a_setting_and_the_settings_program_is_headless_safe() {
    require_e2e();
    let n = Noeul::new();
    let shown = n.ok(&["config", "show"]);
    assert_eq!(
        shown["settings"]["pins"]["double_click"], "close",
        "default"
    );
    n.ok(&["config", "set", "pins.double_click", "none"]);
    assert_eq!(
        n.ok(&["config", "show"])["settings"]["pins"]["double_click"],
        "none"
    );
    let (code, bad) = n.json(&["config", "set", "pins.double_click", "maybe"]);
    assert_eq!(code, 2, "{bad:#}");
    assert_eq!(bad["error"]["code"], "usage");
    let (code, start) = n.json(&["start", "--no-tray"]);
    assert_eq!(code, 0, "{start:#}");
    let st = n.ok(&["status"]);
    assert_eq!(st["settings"]["pins"]["double_click"], "none", "{st:#}");
    n.ok(&["config", "set", "pins.double_click", "close"]);
    assert_eq!(
        n.ok(&["status"])["settings"]["pins"]["double_click"],
        "close",
        "the running instance read it again"
    );
    n.ok(&["quit", "--yes"]);

    // The settings window's program never opens a window for --help / --version.
    let settings = bin_debug("noeul-settings");
    for (arg, code) in [("--help", 0), ("--version", 0), ("--bogus", 2)] {
        let out = Command::new(&settings)
            .arg(arg)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(code), "{arg}");
    }
}

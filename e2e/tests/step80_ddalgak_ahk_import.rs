//! E2E — Step 80: importing an AutoHotkey v2 script (docs/DDALGAK_AHK_MIGRATION.md, ADR 0081).
//! Requirements: `DDALGAK-FR-063`, `DDALGAK-SEC-013`, `KIM-FR-012`.
//!
//! Scenario (release `ddalgak`, isolated profile, no app): an agent previews the repository's
//! convertible fixture (`profile import file.ahk --dry-run --json`): 18 rules, a report with every
//! line converted and none to migrate by hand, nothing written. The real import adds one profile,
//! never active, every rule off and without consent; `rule check` reports only the consent the
//! rules need before they can be turned on. The manual fixture is refused (`import_invalid`) with
//! nothing written, and its dry run names each line (with column) that needs manual migration. A
//! script that would write a file and start a shell is imported as a report only: the file never
//! appears and no process is started.
//!
//! Machine safety: sandboxed `%APPDATA%`/`%LOCALAPPDATA%`; no AutoHotkey, no script and no command
//! is ever run; no window appears.
#![cfg(windows)]

#[path = "../../xtask/src/jsonschema.rs"]
mod jsonschema;

use kim_e2e::{TempDir, bin, require_e2e};
use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/contracts/examples/ddalgak-ahk-v1")
}

fn schema() -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/contracts/schemas/ddalgak-cli-v1.schema.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
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
        let errors = jsonschema::Validator::new(&schema())
            .validate(&doc)
            .unwrap();
        assert!(errors.is_empty(), "{args:?}: {errors:?}\n{doc:#}");
        assert!(out.stderr.is_empty(), "{args:?}: stderr with --json");
        (out.status.code().unwrap_or(-1), doc)
    }

    fn rules_file(&self) -> PathBuf {
        self.sb.path().join("Roaming/mouv/ddalgak/rules.json")
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn step80_an_autohotkey_script_is_previewed_imported_off_and_never_run() {
    require_e2e();
    let sb = TempDir::new("step80-ahk");
    std::fs::create_dir_all(sb.path().join("Roaming")).unwrap();
    std::fs::create_dir_all(sb.path().join("Local")).unwrap();
    let e = Env { sb: &sb };
    let good = fixtures().join("01-convertible.ahk").display().to_string();
    let bad = fixtures().join("02-manual.ahk").display().to_string();

    // Preview: the report, nothing written.
    let (code, d) = e.run(&["profile", "import", &good, "--dry-run"]);
    assert_eq!(code, 0, "{d:#}");
    let r = &d["data"]["ahk"];
    assert_eq!(
        (r["rules"].as_u64(), r["manual"].as_u64()),
        (Some(18), Some(0)),
        "{r:#}"
    );
    assert!(!e.rules_file().exists(), "a preview writes nothing");

    // Import: one profile, not active, every rule off and without consent.
    let (code, d) = e.run(&["profile", "import", &good]);
    assert_eq!(code, 0, "{d:#}");
    let doc: Value = serde_json::from_slice(&std::fs::read(e.rules_file()).unwrap()).unwrap();
    let ahk = doc["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["title"] == "Imported from 01-convertible.ahk")
        .expect("imported");
    assert_ne!(doc["active_profile"], ahk["id"], "never switched to");
    let rules = ahk["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 18);
    assert!(
        rules
            .iter()
            .all(|r| r["enabled"] == false && r.get("consent").is_none())
    );
    let (_, check) = e.run(&["rule", "check"]);
    assert_eq!(check["data"]["summary"]["errors"], 0, "{check:#}");

    // The manual fixture: refused, nothing written; the dry run names each line and column.
    let before = std::fs::read(e.rules_file()).unwrap();
    let (code, d) = e.run(&["profile", "import", &bad]);
    assert_eq!(
        (code, d["error"]["code"].as_str()),
        (4, Some("import_invalid")),
        "{d:#}"
    );
    assert_eq!(std::fs::read(e.rules_file()).unwrap(), before);
    let (_, d) = e.run(&["profile", "import", &bad, "--dry-run"]);
    let manual: Vec<(u64, u64)> = d["data"]["ahk"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["level"] == "manual")
        .map(|x| (x["line"].as_u64().unwrap(), x["column"].as_u64().unwrap()))
        .collect();
    assert_eq!(manual.len(), 14, "{manual:?}");
    assert!(
        manual.contains(&(9, 1)) && manual.contains(&(20, 5)),
        "{manual:?}"
    );

    // A script with side effects is only read: nothing it names happens.
    let marker = sb.path().join("written-by-ahk.txt");
    let evil = sb.path().join("evil.ahk");
    std::fs::write(
        &evil,
        format!(
            "F13::FileAppend \"x\", \"{m}\"\nF14::Run \"cmd.exe /c echo x > {m}\"\n^!x::DllCall(\"kernel32\\\\ExitProcess\", \"UInt\", 0)\n",
            m = marker.display()
        ),
    )
    .unwrap();
    let (_, d) = e.run(&[
        "profile",
        "import",
        &evil.display().to_string(),
        "--dry-run",
    ]);
    assert_eq!(d["data"]["ahk"]["rules"], 0);
    assert_eq!(d["data"]["ahk"]["manual"], 3);
    assert!(!marker.exists(), "nothing in the script ran");
}

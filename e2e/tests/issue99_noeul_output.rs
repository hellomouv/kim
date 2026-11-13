//! E2E — #99 (follow-up of Step 63): Noeul's output follow-ups (docs/NOEUL_SNIPASTE_MATRIX.md
//! `OUT-07`, `CLI-02`, `HIST-06`, `OUT-06`). Requirements: NOEUL-FR-015, NOEUL-FR-031,
//! NOEUL-FR-051, NOEUL-SEC-005.
//!
//! The debug `noeul` with the test hook `NOEUL_DEV_SYNTHETIC_DESKTOP=1` (a synthetic desktop far
//! off-screen; the real screen, displays and clipboard are never read or changed): a script sets
//! `[output] jpeg_quality`, captures a region to a `.jpg` file (JPEG at that quality) and to a
//! `.png` file (PNG), and captures again with `--delay 1` (it waits). The new settings show in
//! `config show` with their defaults off. A captured pin then gets a grayscale view, sharp pixels
//! and the normal z-order through `pin set`, and `pin list` reports them. No tray icon, no visible window, no hotkey registered,
//! no input synthesized; the instance is quit (or ended by a guard).
#![cfg(windows)]

#[path = "../../xtask/src/jsonschema.rs"]
mod jsonschema;

use kim_e2e::{TempDir, bin_debug, require_e2e};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
        let sb = TempDir::new("issue99");
        let cfg = sb.path().join("Roaming/mouv/noeul");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(sb.path().join("Local")).unwrap();
        std::fs::create_dir_all(sb.path().join("Temp")).unwrap();
        // Both default global actions disabled: nothing is ever registered.
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
            endpoint: format!("noeul-e2e-issue99-{}", std::process::id()),
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
fn issue99_capture_writes_jpeg_or_png_and_waits_for_its_delay() {
    require_e2e();
    let n = Noeul::new();
    let shown = n.ok(&["config", "show"]);
    assert_eq!(
        shown["settings"]["output"],
        json!({"format": "png", "jpeg_quality": 90, "copy_as_file": "off"})
    );
    assert_eq!(shown["settings"]["history"]["auto_save"], "off");
    n.ok(&["config", "set", "output.jpeg_quality", "60"]);
    let (code, start) = n.json(&["start", "--no-tray"]);
    assert_eq!(code, 0, "{start:#}");
    let st = n.ok(&["status"]);
    assert_eq!(
        st["capture_backend"], "synthetic",
        "the debug test hook is on"
    );
    let region = "-20000,-20000,64,48";
    let jpg = n.sb.path().join("shot.jpg");
    let png = n.sb.path().join("shot.png");
    for f in [&jpg, &png] {
        let f = f.display().to_string();
        n.ok(&["capture", "--region", region, "--output", &f]);
    }
    let j = std::fs::read(&jpg).unwrap();
    assert_eq!(&j[..3], &[0xFF, 0xD8, 0xFF], "a JPEG file");
    assert_eq!(&std::fs::read(&png).unwrap()[1..4], b"PNG");
    let delayed = n.sb.path().join("delayed.png").display().to_string();
    let t = Instant::now();
    n.ok(&[
        "capture", "--region", region, "--output", &delayed, "--delay", "1",
    ]);
    assert!(
        t.elapsed() >= Duration::from_millis(950),
        "{:?}",
        t.elapsed()
    );
    let t = Instant::now();
    n.ok(&[
        "capture",
        "--region",
        region,
        "--pin",
        "--delay",
        "5",
        "--dry-run",
    ]);
    assert!(
        t.elapsed() < Duration::from_secs(4),
        "a dry run never waits"
    );
    // #99 (`PIN-21`, `PIN-27`): a pin's view filter, sharp pixels and z-order, set headlessly.
    let pinned = n.ok(&["capture", "--region", region, "--pin"]);
    let id = pinned["pin"]["id"]
        .as_str()
        .map_or_else(|| "pin-1".to_string(), str::to_string);
    let set = n.ok(&[
        "pin",
        "set",
        &id,
        "--filter",
        "grayscale",
        "--sharp",
        "on",
        "--topmost",
        "off",
    ]);
    assert_eq!(set["changed"], json!([id.as_str()]), "{set:#}");
    let list = n.ok(&["pin", "list"]);
    let pin = list["pins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(
        (&pin["filter"], &pin["sharp"], &pin["topmost"]),
        (&json!("grayscale"), &json!(true), &json!(false)),
        "{pin:#}"
    );
    n.ok(&["quit", "--yes"]);
}

/// #99 (`SNIP-12`): the typed size and fixed ratio of the capture overlay is a bindable action,
/// discoverable headlessly: `hotkeys list` shows `selection.size` on `T` in the
/// `capture-overlay` context, and `hotkeys show` names it. (The overlay itself is checked by
/// `crates/noeul/src/overlay.rs` tests and on the real desktop; nothing opens here.)
#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn issue99_the_size_entry_is_a_listed_overlay_action() {
    require_e2e();
    let n = Noeul::new();
    let (code, list) = n.json(&["hotkeys", "list"]);
    assert_eq!(code, 0, "{list:#}");
    valid("hotkeys-cli-v1", &list);
    let size = list["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "selection.size")
        .cloned()
        .unwrap_or_else(|| panic!("selection.size is listed: {list:#}"));
    assert_eq!(size["scope"], "context:capture-overlay", "{size:#}");
    assert!(size.to_string().contains("\"T\""), "bound to T: {size:#}");
    let (code, show) = n.json(&["hotkeys", "show", "selection.size"]);
    assert_eq!(code, 0, "{show:#}");
    valid("hotkeys-cli-v1", &show);
}

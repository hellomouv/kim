//! Step 60 (NOEUL-FR-050, NOEUL-FR-051, NOEUL-FR-066, NOEUL-SEC-005, ADR 0074) with the real
//! `noeul.exe` on the debug build's synthetic desktop (far off-screen, nothing read from the
//! screen, nothing takes focus), in a sandboxed `APPDATA`/`LOCALAPPDATA`:
//!
//! - by default captures are kept in memory only — no file appears in the data folder;
//! - `noeul history list|pin|forget|clear` over IPC, every answer valid against the IPC schema;
//! - with `history.disk on` and `pins.backup on` (`noeul config set`), captures and pins are
//!   written to the user-only data folder; after `noeul quit --yes` and a new start the history
//!   is back, the pins are offered and `noeul pin restore` brings them back once (a second
//!   restore duplicates nothing);
//! - `noeul history clear` and `noeul pin backup clear` also work with no Noeul running.
#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use common::*;
use serde_json::Value;
use std::time::{Duration, Instant};

fn assert_valid(name: &str, v: &Value) {
    let e = validate(name, v);
    assert!(
        e.is_empty(),
        "violates {name}: {e:?}
{v:#}"
    );
}

fn data(sb: &Sandbox) -> std::path::PathBuf {
    sb.root.join("Local").join("mouv").join("noeul")
}

fn ok(sb: &Sandbox, args: &[&str]) -> Value {
    let (code, v) = sb.json(args);
    assert_eq!(code, 0, "{args:?}: {v:#}");
    v["data"].clone()
}

fn region(sb: &Sandbox, dx: i64) -> String {
    let st = ok(sb, &["status"]);
    let m = st["desktop"]["monitors"]
        .as_array()
        .and_then(|m| m.iter().find(|m| m["primary"] == true))
        .unwrap()
        .clone();
    let (x, y) = (
        m["rect"]["x"].as_i64().unwrap(),
        m["rect"]["y"].as_i64().unwrap(),
    );
    format!("{},{},6,4", x + 20 + dx, y + 20)
}

#[test]
fn history_and_pin_backup_over_the_cli_and_across_a_restart() {
    let mut sb = Sandbox::synthetic("keep", Keys::Off);
    sb.instance();
    // Memory only by default.
    ok(&sb, &["capture", "--region", &region(&sb, 0), "--pin"]);
    let l = ok(&sb, &["history", "list"]);
    assert_valid("noeul-ipc-v1#history_list_result", &l);
    assert_eq!(
        (l["count"].as_u64(), l["disk"].as_bool()),
        (Some(1), Some(false))
    );
    assert!(
        !data(&sb).join("history").exists(),
        "nothing on disk by default"
    );
    assert!(!data(&sb).join("pins").exists());
    let p = ok(&sb, &["history", "pin"]);
    assert_valid("noeul-ipc-v1#history_pin_result", &p);
    assert_eq!(p["pin"], "pin-2");
    let (code, e) = sb.json(&["history", "pin", "99"]);
    assert_eq!(
        (code, e["error"]["code"].as_str()),
        (3, Some("history_entry_not_found"))
    );
    let (code, e) = sb.json(&["history", "clear"]);
    assert_eq!(
        (code, e["error"]["code"].as_str()),
        (7, Some("confirmation_required"))
    );

    // Opt in to the disk history and the pin backup.
    ok(&sb, &["config", "set", "history.disk", "on"]);
    ok(&sb, &["config", "set", "pins.backup", "on"]);
    // The settings watcher reloads; wait until the instance reports the new settings.
    let until = Instant::now() + Duration::from_secs(10);
    while ok(&sb, &["pin", "backup", "status"])["enabled"] != true {
        assert!(Instant::now() < until, "the settings were not reloaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    ok(&sb, &["capture", "--region", &region(&sb, 10), "--pin"]);
    ok(
        &sb,
        &[
            "pin",
            "set",
            "pin-3",
            "--opacity",
            "60",
            "--move-to-group",
            "work",
        ],
    );
    // The backup is written a second after the pins are still.
    let until = Instant::now() + Duration::from_secs(10);
    while !data(&sb).join("pins").join("backup.json").is_file() {
        assert!(Instant::now() < until, "no backup was written");
        std::thread::sleep(Duration::from_millis(100));
    }
    let s = ok(&sb, &["pin", "backup", "save"]);
    assert_valid("noeul-ipc-v1#backup_save_result", &s);
    assert_eq!(s["saved"], 3);
    let pngs = |dir: std::path::PathBuf| {
        std::fs::read_dir(dir).map_or(0, |d| {
            d.flatten()
                .filter(|f| f.file_name().to_string_lossy().ends_with(".png"))
                .count()
        })
    };
    assert_eq!(
        pngs(data(&sb).join("history")),
        1,
        "the capture after opting in"
    );
    assert!(
        sb.temp_dir().read_dir().unwrap().next().is_none(),
        "no temporary files"
    );

    // Restart: the history is back; the pins are offered, not restored by themselves.
    let (code, _) = sb.json(&["quit", "--yes"]);
    assert_eq!(code, 0);
    sb.instance();
    let st = ok(&sb, &["status"]);
    assert_eq!(st["pins"]["open"], 0, "nothing restored by itself");
    let l = ok(&sb, &["history", "list"]);
    assert_eq!(
        (l["count"].as_u64(), l["entries"][0]["on_disk"].as_bool()),
        (Some(1), Some(true))
    );
    let b = ok(&sb, &["pin", "backup", "status"]);
    assert_valid("noeul-ipc-v1#backup_status_result", &b);
    assert_eq!(
        (b["pins"].as_u64(), b["restorable"].as_u64()),
        (Some(3), Some(3))
    );
    let r = ok(&sb, &["pin", "restore"]);
    assert_valid("noeul-ipc-v1#backup_restore_result", &r);
    assert_eq!(r["restored"].as_array().unwrap().len(), 3, "{r:#}");
    let list = ok(&sb, &["pin", "list"]);
    let work: Vec<&Value> = list["pins"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["group"] == "work")
        .collect();
    assert_eq!(work.len(), 1, "{list:#}");
    assert_eq!(work[0]["opacity"], 60);
    let r = ok(&sb, &["pin", "restore"]);
    assert_eq!(
        (
            r["restored"].as_array().unwrap().len(),
            r["already_open"].as_u64()
        ),
        (0, Some(3))
    );
    assert_eq!(ok(&sb, &["status"])["pins"]["open"], 3, "no duplicates");

    // Without a running Noeul, the private data can still be removed.
    let (code, _) = sb.json(&["quit", "--yes"]);
    assert_eq!(code, 0);
    let b = ok(&sb, &["pin", "backup", "status"]);
    assert_eq!(b["pins"], 3, "read from the file");
    let c = ok(&sb, &["history", "clear", "--yes"]);
    assert_valid("noeul-ipc-v1#history_clear_result", &c);
    assert_eq!(c["disk_cleared"], 1);
    assert_eq!(pngs(data(&sb).join("history")), 0);
    let c = ok(&sb, &["pin", "backup", "clear", "--yes"]);
    assert_valid("noeul-ipc-v1#backup_clear_result", &c);
    assert_eq!(c["removed"], 3);
    assert!(!data(&sb).join("pins").exists());
    sb.stop_all();
}

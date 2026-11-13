//! E2E — #98 T2 (follow-up of Step 36): Subak's keyboard and line-editing settings
//! (docs/TABBY_FEATURE_MATRIX.md `CORE-57`, `CORE-58`, `CORE-61`, `CORE-62`). `SUBAK-FR-013`,
//! `KIM-FR-013`.
//!
//! The way a user or an agent sets them up, with the released `subak` in a private `APPDATA`
//! sandbox (no Host, no window): `hotkeys list` offers `terminal.insert-newline` on
//! `Alt+Shift+Enter` and the six line-editing actions without keys; `hotkeys set` binds one and
//! `hotkeys check` accepts it; `config show` reports `terminal.alt_is_meta` (true) and
//! `terminal.backspace` (`del`) as defaults; `config set` changes both (`--dry-run` first, which
//! writes nothing); a value outside `del`/`ctrl-h`/`delete` is refused with a usage error and
//! changes nothing. The window's use of them is tested in
//! `crates/subak/tests/gui_window.rs::line_editing_insert_newline_backspace_and_alt_as_meta`.
#![cfg(windows)]

use kim_e2e::{TempDir, bin, run_sandboxed};
use serde_json::Value;

fn json(sb: &TempDir, args: &[&str]) -> (i32, Value) {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    let out = run_sandboxed(&bin("subak"), &a, sb);
    let doc = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap_or(-1), doc)
}

fn setting(doc: &Value, key: &str) -> Value {
    doc["data"]["settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["key"] == key)
        .unwrap_or_else(|| panic!("no {key} in {doc}"))
        .clone()
}

fn keys_of(list: &Value, id: &str) -> Vec<String> {
    let a = list["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap_or_else(|| panic!("no action {id}"));
    a["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["keys"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn issue98_line_editing_actions_and_keyboard_settings() {
    let sb = TempDir::new("issue98-t2");
    // The actions.
    let (code, list) = json(&sb, &["hotkeys", "list"]);
    assert_eq!(code, 0, "{list}");
    assert_eq!(
        keys_of(&list, "terminal.insert-newline"),
        ["Alt+Shift+Enter"]
    );
    for id in [
        "line.home",
        "line.end",
        "line.previous-word",
        "line.next-word",
        "line.delete-previous-word",
        "line.delete-line",
    ] {
        assert!(keys_of(&list, id).is_empty(), "{id} has no default key");
    }
    let (code, doc) = json(&sb, &["hotkeys", "set", "line.home", "Ctrl+Shift+F9"]);
    assert_eq!(code, 0, "{doc}");
    let (code, doc) = json(&sb, &["hotkeys", "check"]);
    assert_eq!(code, 0, "{doc}");
    let (_, list) = json(&sb, &["hotkeys", "list"]);
    assert_eq!(keys_of(&list, "line.home"), ["Ctrl+Shift+F9"]);
    // The settings: defaults, a dry run, the change, a refused value.
    let (code, show) = json(&sb, &["config", "show"]);
    assert_eq!(code, 0, "{show}");
    assert_eq!(setting(&show, "terminal.alt_is_meta")["value"], true);
    assert_eq!(setting(&show, "terminal.backspace")["value"], "del");
    let (code, doc) = json(
        &sb,
        &["config", "set", "terminal.backspace", "ctrl-h", "--dry-run"],
    );
    assert_eq!(code, 0, "{doc}");
    let (_, show) = json(&sb, &["config", "show"]);
    assert_eq!(
        setting(&show, "terminal.backspace")["value"],
        "del",
        "a dry run writes nothing"
    );
    for (k, v) in [
        ("terminal.backspace", "ctrl-h"),
        ("terminal.alt_is_meta", "false"),
    ] {
        let (code, doc) = json(&sb, &["config", "set", k, v]);
        assert_eq!(code, 0, "{doc}");
    }
    let (code, doc) = json(&sb, &["config", "set", "terminal.backspace", "bs"]);
    assert_ne!(code, 0, "{doc}");
    let (_, show) = json(&sb, &["config", "show"]);
    assert_eq!(setting(&show, "terminal.backspace")["value"], "ctrl-h");
    assert_eq!(setting(&show, "terminal.backspace")["source"], "file");
    assert_eq!(setting(&show, "terminal.alt_is_meta")["value"], false);
}

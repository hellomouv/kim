//! Step 61 (NOEUL-FR-101, NOEUL-FR-111, NOEUL-SEC-012, ADR 0075): the settings window opens only
//! when asked (the tray's "Settings…", `settings.open`), a dry run opens nothing, and while it
//! records keys a global hotkey does nothing — recording `F1` never starts a capture.

use super::tests::{FakeShell, overlay_app};
use super::*;

#[test]
fn the_settings_window_opens_only_when_asked() {
    let (mut a, _syn, _dir) = overlay_app("settings-open");
    assert_eq!(a.shell.settings_opened, 0, "nothing opens it by itself");
    assert!(a.tray_menu().contains(&MenuItem::Settings));
    a.tray_command(MenuItem::Settings);
    assert_eq!(a.shell.settings_opened, 1);
    let v = a
        .request("settings.open", &json!({"dry_run": true}))
        .unwrap();
    assert_eq!(
        (v["opened"].as_bool(), a.shell.settings_opened),
        (Some(false), 1)
    );
    let v = a.request("settings.open", &json!({})).unwrap();
    assert_eq!(
        (v["opened"].as_bool(), a.shell.settings_opened),
        (Some(true), 2)
    );
    // Other requests never open it.
    for m in ["status", "pin.list", "history.list", "backup.status"] {
        a.request(m, &json!({})).unwrap();
    }
    assert_eq!(a.shell.settings_opened, 2);
}

fn press(a: &mut App<FakeShell>, action: &str) {
    let id = (1..=64)
        .map(kim_hotkeys::register::HotkeyId)
        .find(|id| a.hotkeys.action_for(*id).as_deref() == Some(action));
    // The test instance has no registrations: drive the action like a hotkey would.
    match id {
        Some(id) => a.hotkey_pressed(id),
        None => a.action(action, None),
    }
}

#[test]
fn recording_keys_never_runs_a_hotkey() {
    let (mut a, _syn, _dir) = overlay_app("settings-rec");
    a.shell.recording = true;
    a.hotkey_pressed(kim_hotkeys::register::HotkeyId(1));
    assert!(!a.overlay_open(), "no capture while recording");
    assert_eq!(
        a.status()["last_action"],
        json!({"action": "hotkey", "result": "recording"})
    );
    a.shell.recording = false;
    press(&mut a, "capture.region");
    assert!(a.overlay_open(), "after recording the keys work again");
}

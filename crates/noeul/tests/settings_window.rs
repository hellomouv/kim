//! Step 61 (NOEUL-FR-101, NOEUL-FR-111, NOEUL-NFR-012, NOEUL-NFR-013, KIM-FR-013, ADR 0075): the real settings window of
//! `noeul.exe` under the debug-build test hook — far off-screen, never activated — driven only
//! with messages to its own standard controls, its results read back through the CLI:
//!
//! - nothing but `noeul settings` opens it (the other commands are headless);
//! - a recorded chord becomes the action's binding (`hotkeys list` shows it), `Escape` cancels;
//! - a binding over a default is allowed and says what it shadows; a conflict with another user
//!   binding is refused in words and "Use anyway" moves the keys (`--replace`);
//! - Disable, Reset and Reset all (two presses, no modal dialog) do what the CLI does;
//! - the double-click gesture and the disk-history switch reach `config.toml`;
//! - the list box, buttons and status are standard controls with their text as their name.
#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use common::*;
use serde_json::Value;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetDlgItem, GetWindowThreadProcessId, IsWindow, PostMessageW,
    SendMessageW, WM_CLOSE, WM_COMMAND,
};

const LB_GETCOUNT: u32 = 0x018B;
const LB_SETCURSEL: u32 = 0x0186;
const CB_SETCURSEL: u32 = 0x014E;
const BM_SETCHECK: u32 = 0x00F1;
const WM_TEST_KEY: u32 = 0x8000 + 0x61;
const LIST: i32 = 100;
const CHANGE: i32 = 101;
const DISABLE: i32 = 102;
const RESET: i32 = 103;
const RESET_ALL: i32 = 104;
const FORCE: i32 = 107;
const DOUBLE: i32 = 110;
const HISTORY_DISK: i32 = 121;
const STATUS: i32 = 130;

fn settings_windows(pid: u32) -> Vec<isize> {
    unsafe extern "system" fn each(h: HWND, data: isize) -> i32 {
        // SAFETY: `data` is the Vec passed below, alive during EnumWindows.
        unsafe { (*(data as *mut Vec<isize>)).push(h as isize) };
        1
    }
    let mut all: Vec<isize> = Vec::new();
    // SAFETY: the callback only touches `all`.
    unsafe { EnumWindows(Some(each), (&raw mut all) as isize) };
    all.into_iter()
        .filter(|&h| {
            let mut owner = 0u32;
            let mut name = [0u16; 64];
            // SAFETY: plain queries.
            unsafe {
                GetWindowThreadProcessId(h as HWND, &raw mut owner);
                let n = GetClassNameW(h as HWND, name.as_mut_ptr(), 64);
                owner == pid && String::from_utf16_lossy(&name[..n as usize]) == "NoeulSettings"
            }
        })
        .collect()
}

fn ctl(w: isize, id: i32) -> HWND {
    // SAFETY: a child of the test instance's window.
    unsafe { GetDlgItem(w as HWND, id) }
}

fn text(h: HWND) -> String {
    let mut buf = [0u16; 512];
    // SAFETY: WM_GETTEXT to a standard control of another process (the system marshals it).
    let n = unsafe { SendMessageW(h, 0x000D, 512, buf.as_mut_ptr() as isize) };
    String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0)])
}

fn click(w: isize, id: i32, code: usize) {
    // SAFETY: posting to the test instance's own window.
    unsafe {
        PostMessageW(
            w as HWND,
            WM_COMMAND,
            (code << 16) | id as usize,
            ctl(w, id) as isize,
        )
    };
}

fn wait(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < until, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn keys_of(sb: &Sandbox, id: &str) -> Vec<String> {
    let (_, l) = sb.json(&["hotkeys", "list"]);
    l["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap()["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["active"] != false)
        .map(|b| b["keys"].as_str().unwrap().to_string())
        .collect()
}

fn index_of(sb: &Sandbox, id: &str) -> usize {
    let (_, l) = sb.json(&["hotkeys", "list"]);
    l["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|a| a["id"] == id)
        .unwrap()
}

fn select(w: isize, i: usize) {
    // SAFETY: our test instance's list box; then the notification its parent acts on.
    unsafe { SendMessageW(ctl(w, LIST), LB_SETCURSEL, i, 0) };
    click(w, LIST, 1);
}

/// Waits until the window is ready for input again (a command and its reload finished: the
/// buttons are enabled).
fn ready(w: isize) {
    // SAFETY: a state query on the test instance's button.
    wait(
        "the window ready",
        || unsafe { IsWindowEnabled(ctl(w, CHANGE)) } != 0,
    );
}

/// Waits until the status line changed from `before` and is not a progress word.
fn status_after(w: isize, before: &str) -> String {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let s = text(ctl(w, STATUS));
        if s != before && !s.ends_with('…') {
            ready(w);
            return s;
        }
        assert!(
            Instant::now() < until,
            "the status line stayed {s:?} (before: {before:?})"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_settings_window_records_disables_resets_and_saves_through_the_cli() {
    let mut sb = Sandbox::synthetic("settings", Keys::NoGlobals);
    let pid = sb.instance();
    // Headless commands never open it.
    for args in [
        ["hotkeys", "list"],
        ["config", "show"],
        ["status", "--json"],
    ] {
        let _ = sb.json(&args[..args.len().min(2)]);
    }
    assert!(
        settings_windows(pid).is_empty(),
        "no window from headless commands"
    );
    let (code, v) = sb.json(&["settings", "--dry-run"]);
    assert_eq!((code, v["data"]["opened"].as_bool()), (0, Some(false)));
    assert!(settings_windows(pid).is_empty());
    let (code, v) = sb.json(&["settings"]);
    assert_eq!(
        (code, v["data"]["opened"].as_bool()),
        (0, Some(true)),
        "{v:#}"
    );
    let w = settings_windows(pid)[0];
    let until = Instant::now() + Duration::from_secs(20);
    // SAFETY: a count query on the test instance's list box.
    while unsafe { SendMessageW(ctl(w, LIST), LB_GETCOUNT, 0, 0) } <= 50 {
        assert!(
            Instant::now() < until,
            "the list never filled; status: {:?}",
            text(ctl(w, STATUS))
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    ready(w);
    assert!(
        text(ctl(w, STATUS)).contains("actions"),
        "{}",
        text(ctl(w, STATUS))
    );
    // Standard controls carry their names (what a screen reader reads).
    assert_eq!(text(ctl(w, CHANGE)), "C&hange…");
    assert_eq!(text(ctl(w, RESET_ALL)), "Reset &all…");

    // Record Shift+Q for capture.private.
    select(w, index_of(&sb, "capture.private"));
    click(w, CHANGE, 0);
    wait("recording", || {
        text(ctl(w, STATUS)).starts_with("Press the new keys")
    });
    let before = text(ctl(w, STATUS));
    // SAFETY: the test hook's recorded-key message: 'Q' with Shift.
    unsafe { PostMessageW(w as HWND, WM_TEST_KEY, 0x51, 4) };
    let s = status_after(w, &before);
    assert!(s.contains("now runs on Shift+Q"), "{s}");
    assert_eq!(keys_of(&sb, "capture.private"), ["Shift+Q"]);

    // Escape cancels a recording; nothing changes.
    click(w, CHANGE, 0);
    wait("recording again", || {
        text(ctl(w, STATUS)).starts_with("Press the new keys")
    });
    // SAFETY: as above, Escape alone.
    unsafe { PostMessageW(w as HWND, WM_TEST_KEY, 0x1B, 0) };
    wait("cancelled", || {
        text(ctl(w, STATUS)).contains("nothing changed")
    });
    ready(w);
    assert_eq!(keys_of(&sb, "capture.private"), ["Shift+Q"]);

    // A conflict with another user binding: refused in words; "Use anyway" moves the keys.
    select(w, index_of(&sb, "detect.cycle"));
    ready(w);
    click(w, CHANGE, 0);
    wait("recording a conflict", || {
        text(ctl(w, STATUS)).starts_with("Press the new keys")
    });
    let before = text(ctl(w, STATUS));
    // SAFETY: as above, Shift+Q again.
    unsafe { PostMessageW(w as HWND, WM_TEST_KEY, 0x51, 4) };
    let s = status_after(w, &before);
    assert!(s.contains("conflicts") && s.contains("Use anyway"), "{s}");
    // SAFETY: a state query on the test instance's button.
    wait(
        "Use anyway enabled",
        || unsafe { IsWindowEnabled(ctl(w, FORCE)) } != 0,
    );
    assert_eq!(
        keys_of(&sb, "detect.cycle"),
        ["E"],
        "refused: nothing changed"
    );
    click(w, FORCE, 0);
    let s = status_after(w, &s);
    assert!(s.contains("now runs on Shift+Q"), "{s}");
    assert_eq!(keys_of(&sb, "detect.cycle"), ["Shift+Q"]);
    assert!(
        keys_of(&sb, "capture.private").is_empty(),
        "moved, not duplicated"
    );

    // Disable, then Reset back to the default.
    let before = text(ctl(w, STATUS));
    click(w, DISABLE, 0);
    let s = status_after(w, &before);
    assert!(s.contains("has no keys now"), "{s}");
    assert!(keys_of(&sb, "detect.cycle").is_empty());
    click(w, RESET, 0);
    let s = status_after(w, &s);
    assert!(s.contains("back to its default"), "{s}");
    assert_eq!(keys_of(&sb, "detect.cycle"), ["E"]);

    // Reset all: the first press asks in words, the second resets.
    click(w, RESET_ALL, 0);
    wait("asked", || {
        text(ctl(w, STATUS)).contains("again to confirm")
    });
    assert!(
        keys_of(&sb, "capture.private").is_empty(),
        "nothing reset yet"
    );
    let before = text(ctl(w, STATUS));
    click(w, RESET_ALL, 0);
    let s = status_after(w, &before);
    assert!(s.contains("Every shortcut is back"), "{s}");
    assert_eq!(keys_of(&sb, "capture.private"), ["P"]);

    // The double-click gesture and the disk-history switch reach config.toml.
    // SAFETY: the test instance's combo box and check box, then their notifications.
    unsafe { SendMessageW(ctl(w, DOUBLE), CB_SETCURSEL, 1, 0) };
    let before = text(ctl(w, STATUS));
    click(w, DOUBLE, 1);
    status_after(w, &before);
    let (_, show) = sb.json(&["config", "show"]);
    assert_eq!(show["data"]["settings"]["gestures"]["double_click"], "copy");
    // SAFETY: as above.
    unsafe { SendMessageW(ctl(w, HISTORY_DISK), BM_SETCHECK, 1, 0) };
    click(w, HISTORY_DISK, 0);
    wait("history.disk on in config.toml", || {
        let (_, show): (i32, Value) = sb.json(&["config", "show"]);
        show["data"]["settings"]["history"]["disk"] == "on"
    });
    ready(w);
    assert_eq!(text(ctl(w, STATUS)), "Saved.");

    // Closed: gone, and the instance keeps running.
    // SAFETY: posting to the test instance's window.
    unsafe { PostMessageW(w as HWND, WM_CLOSE, 0, 0) };
    // SAFETY: a validity query.
    wait("closed", || unsafe { IsWindow(w as HWND) } == 0);
    let (code, _) = sb.json(&["health"]);
    assert_eq!(code, 0);
    sb.stop_all();
}

/// A global key changed in the window is registered by the running instance at once (it reloads
/// the file the CLI wrote) and released from the old chord; changing it back restores the old
/// registration — the F1 / F3 path, on obscure test chords only (never F1 on this machine).
#[test]
fn a_global_key_changed_in_the_window_is_registered_live_and_back() {
    let mut sb = Sandbox::synthetic(
        "settings-live",
        Keys::Both {
            capture: "Ctrl+Alt+Shift+F20",
            paste: "Ctrl+Alt+Shift+F21",
        },
    );
    let pid = sb.instance();
    let registration = |sb: &Sandbox, id: &str| -> (Vec<String>, Vec<String>) {
        let (_, l) = sb.json(&["hotkeys", "list"]);
        let a = l["data"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .unwrap()
            .clone();
        let b = a["bindings"].as_array().unwrap();
        (
            b.iter()
                .map(|b| b["keys"].as_str().unwrap().to_string())
                .collect(),
            b.iter()
                .map(|b| b["registration"].as_str().unwrap_or("").to_string())
                .collect(),
        )
    };
    assert_eq!(
        registration(&sb, "capture.region"),
        (
            vec!["Ctrl+Alt+Shift+F20".to_string()],
            vec!["registered".to_string()]
        )
    );
    sb.json(&["settings"]);
    let w = settings_windows(pid)[0];
    ready(w);
    select(w, index_of(&sb, "capture.region"));
    ready(w);
    for (vk, keys) in [
        (0x75u32 + 17, "Ctrl+Alt+Shift+F23"),
        (0x70 + 19, "Ctrl+Alt+Shift+F20"),
    ] {
        click(w, CHANGE, 0);
        wait("recording", || {
            text(ctl(w, STATUS)).starts_with("Press the new keys")
        });
        assert!(obscure(keys));
        // SAFETY: the test hook's recorded-key message with Ctrl, Alt and Shift.
        unsafe { PostMessageW(w as HWND, WM_TEST_KEY, vk as usize, 7) };
        wait("registered live", || {
            registration(&sb, "capture.region")
                == (vec![keys.to_string()], vec!["registered".to_string()])
        });
        ready(w);
        // F3's twin stays as it was.
        assert_eq!(registration(&sb, "paste.float").1, ["registered"]);
    }
    // SAFETY: posting to the test instance's window.
    unsafe { PostMessageW(w as HWND, WM_CLOSE, 0, 0) };
    sb.stop_all();
}

const GROUPS: i32 = 150;
const GROUP_SWITCH: i32 = 151;
const GROUP_HIDE: i32 = 153;

fn current_group(sb: &Sandbox) -> String {
    let (_, l) = sb.json(&["pin", "group", "list"]);
    l["data"]["current"].as_str().unwrap_or("").to_string()
}

/// #99 (`PIN-15`): the settings window lists the running instance's pin groups (through the
/// instance's own pipe, here the test endpoint) and its buttons switch to a group and hide a
/// group's pins through the CLI.
#[test]
fn the_settings_window_manages_the_pin_groups() {
    let mut sb = Sandbox::synthetic("settings-groups", Keys::NoGlobals);
    let pid = sb.instance();
    // A pin in each group (groups without pins are not listed unless current).
    let (code, v) = sb.json(&["capture", "--region", "-20000,-20000,64,48", "--pin"]);
    assert_eq!(code, 0, "{v:#}");
    let (code, v) = sb.json(&["pin", "group", "switch", "work"]);
    assert_eq!(code, 0, "{v:#}");
    let (code, v) = sb.json(&["capture", "--region", "-20000,-20000,32,24", "--pin"]);
    assert_eq!(code, 0, "{v:#}");
    assert_eq!(current_group(&sb), "work");
    let (code, v) = sb.json(&["settings"]);
    assert_eq!(code, 0, "{v:#}");
    let w = settings_windows(pid)[0];
    // SAFETY: a count query on the test instance's list box.
    wait("the groups listed", || unsafe {
        SendMessageW(ctl(w, GROUPS), LB_GETCOUNT, 0, 0) >= 2
    });
    ready(w);
    // The groups are in file order: `default` first. Switch to it.
    // SAFETY: our test instance's list box; then the notification its parent acts on.
    unsafe { SendMessageW(ctl(w, GROUPS), LB_SETCURSEL, 0, 0) };
    click(w, GROUPS, 1);
    let before = text(ctl(w, STATUS));
    click(w, GROUP_SWITCH, 0);
    let s = status_after(w, &before);
    assert!(s.contains("Switched to the group default"), "{s}");
    assert_eq!(current_group(&sb), "default");
    // Hide the pins of `work` (its row is the second).
    // SAFETY: our test instance's list box.
    unsafe { SendMessageW(ctl(w, GROUPS), LB_SETCURSEL, 1, 0) };
    click(w, GROUPS, 1);
    let before = text(ctl(w, STATUS));
    click(w, GROUP_HIDE, 0);
    let s = status_after(w, &before);
    assert!(s.contains("group work were hidden"), "{s}");
    // SAFETY: closing the test instance's window.
    unsafe { PostMessageW(w as HWND, WM_CLOSE, 0, 0) };
}

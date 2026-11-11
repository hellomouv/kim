//! E2E — Step 61: Noeul's settings window and shortcut preferences (docs/adr/0075-noeul-settings-window.md).
//! Requirements: NOEUL-FR-100, NOEUL-FR-101, NOEUL-FR-111, NOEUL-NFR-012, NOEUL-NFR-013, KIM-FR-013.
//!
//! Scenario A (release `noeul`, nothing running): the settings window is the one command besides
//! the launchers that opens a window (`capabilities`), and it needs a running Noeul
//! (`noeul_not_running`); the window's own commands — `hotkeys set|disable|reset|export|import`,
//! `config set gestures.double_click` — work headlessly with dry runs and leave no window.
//!
//! Scenario B (debug `noeul`, test hook: synthetic desktop far off-screen, nothing activated):
//! the user opens the settings window, selects "Mark this capture private", records Shift+Q (the
//! test hook's recorded-key message), and `hotkeys list` shows it; Escape cancels a recording;
//! closing the window leaves the instance running.
//!
//! No tray icon, no visible window, no hotkey registered (global actions disabled in the profile),
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
fn a_the_settings_command_and_its_cli_twins_are_headless() {
    let n = Noeul::new("s61a", bin("noeul"), false);
    let caps = n.ok(&["capabilities"]);
    let settings = caps["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "settings")
        .unwrap()
        .clone();
    assert_eq!(settings["opens_window"], true);
    let (code, e) = n.json(&["settings"]);
    assert_eq!(
        (code, e["error"]["code"].as_str()),
        (3, Some("noeul_not_running"))
    );
    let hk =
        n.sb.path()
            .join("Roaming")
            .join("mouv")
            .join("noeul")
            .join("hotkeys.json");
    let before = std::fs::read(&hk).unwrap();
    let (code, d) = n.json(&["hotkeys", "set", "capture.private", "Shift+Q", "--dry-run"]);
    assert_eq!(
        (code, d["data"]["written"].as_bool()),
        (0, Some(false)),
        "{d:#}"
    );
    assert_eq!(
        std::fs::read(&hk).unwrap(),
        before,
        "a dry run writes nothing"
    );
    n.ok(&["hotkeys", "set", "capture.private", "Shift+Q"]);
    n.ok(&["hotkeys", "disable", "capture.private"]);
    n.ok(&["hotkeys", "reset", "capture.private"]);
    let exported = n.ok(&["hotkeys", "export"]);
    assert_eq!(exported["tool"], "noeul");
    n.ok(&["config", "set", "gestures.double_click", "copy"]);
    assert_eq!(
        n.ok(&["config", "show"])["settings"]["gestures"]["double_click"],
        "copy"
    );
    n.ok(&["config", "set", "gestures.double_click", "pin"]);
}

#[cfg(windows)]
#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn b_the_user_records_a_key_in_the_settings_window_off_screen() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetDlgItem, GetWindowThreadProcessId, PostMessageW,
        SendMessageW, WM_CLOSE, WM_COMMAND,
    };
    let n = Noeul::new("s61b", kim_e2e::bin_debug("noeul"), true);
    n.start();
    let pid = n.ok(&["status"])["instance"]["pid"].as_u64().unwrap() as u32;
    assert_eq!(n.ok(&["settings"])["opened"], true);
    let find = || -> Option<isize> {
        unsafe extern "system" fn each(h: HWND, data: isize) -> i32 {
            // SAFETY: `data` is the Vec below, alive during EnumWindows.
            unsafe { (*(data as *mut Vec<isize>)).push(h as isize) };
            1
        }
        let mut all: Vec<isize> = Vec::new();
        // SAFETY: the callback only touches `all`.
        unsafe { EnumWindows(Some(each), (&raw mut all) as isize) };
        all.into_iter().find(|&h| {
            let (mut owner, mut name) = (0u32, [0u16; 64]);
            // SAFETY: plain queries.
            unsafe {
                GetWindowThreadProcessId(h as HWND, &raw mut owner);
                let k = GetClassNameW(h as HWND, name.as_mut_ptr(), 64);
                owner == pid && String::from_utf16_lossy(&name[..k as usize]) == "NoeulSettings"
            }
        })
    };
    let w = find().expect("the settings window");
    // SAFETY: queries and posts to this scenario's own off-screen window and its controls.
    let ctl = |id: i32| unsafe { GetDlgItem(w as HWND, id) };
    let text = |id: i32| {
        let mut buf = [0u16; 512];
        // SAFETY: WM_GETTEXT to a standard control (marshalled by the system).
        let k = unsafe { SendMessageW(ctl(id), 0x000D, 512, buf.as_mut_ptr() as isize) };
        String::from_utf16_lossy(&buf[..usize::try_from(k).unwrap_or(0)])
    };
    let until = Instant::now() + Duration::from_secs(20);
    // SAFETY: LB_GETCOUNT on the list box.
    while unsafe { SendMessageW(ctl(100), 0x018B, 0, 0) } < 50 || !text(130).contains("actions") {
        assert!(
            Instant::now() < until,
            "the window never filled: {:?}",
            text(130)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let (_, list) = n.json(&["hotkeys", "list"]);
    let idx = list["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|a| a["id"] == "capture.private")
        .unwrap();
    // SAFETY: select the row, notify, press Change, record Shift+Q.
    unsafe {
        SendMessageW(ctl(100), 0x0186, idx, 0);
        PostMessageW(w as HWND, WM_COMMAND, (1 << 16) | 100, ctl(100) as isize);
        PostMessageW(w as HWND, WM_COMMAND, 101, ctl(101) as isize);
    }
    let until = Instant::now() + Duration::from_secs(20);
    while !text(130).starts_with("Press the new keys") {
        assert!(Instant::now() < until, "not recording: {:?}", text(130));
        std::thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: the test hook's recorded-key message: Q with Shift.
    unsafe { PostMessageW(w as HWND, 0x8000 + 0x61, 0x51, 4) };
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let (_, list) = n.json(&["hotkeys", "list"]);
        let keys = list["data"]["actions"][idx]["bindings"][0]["keys"].clone();
        if keys == "Shift+Q" {
            break;
        }
        assert!(Instant::now() < until, "not bound: {keys}");
        std::thread::sleep(Duration::from_millis(100));
    }
    // SAFETY: as above.
    unsafe { PostMessageW(w as HWND, WM_CLOSE, 0, 0) };
    let until = Instant::now() + Duration::from_secs(10);
    while find().is_some() {
        assert!(Instant::now() < until, "the window did not close");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        n.ok(&["health"])["identity"]["image"],
        "noeul.exe",
        "the instance keeps running"
    );
}

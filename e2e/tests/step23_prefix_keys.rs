//! E2E — Step 23: the `Ctrl+B` prefix and key binding engine (docs/SUBAK_GUI.md §15,
//! HOTKEY_CONTRACT §13.1, ADR 0057). `SUBAK-FR-033`, `SUBAK-FR-034`.
//!
//! Scenario A (debug `subak-gui`, off screen): a user drives the shown Terminal's internal windows
//! and panes with the prefix — `Ctrl+B c` (a window, not a tab), `n`/`p`/`0`, `%` (a split),
//! `z`, `Ctrl+B Ctrl+B` (a literal `Ctrl+B` in the program), `Ctrl+B Ctrl+T` (typed, no new tab),
//! `F1` (the keys), `Ctrl+B d` (the window lets go; every Terminal keeps running).
//!
//! Scenario B (release `subak`): the console client's prefix and detach key follow the user's
//! `hotkeys.json` (`Ctrl+A`, `client.detach` on `@prefix char:q`).
//!
//! The window is the debug build's off-screen test window driven by its own messages
//! (`WM_COPYDATA` test commands); nothing is shown or synthesized on the desktop. Windows only;
//! private endpoints and `APPDATA` sandboxes; everything started is stopped.
#![cfg(windows)]
#![allow(unsafe_code)]
// The window harness is the Step 17 scenario's; not every helper is used here.
#![allow(dead_code)]

use kim_e2e::{TempDir, bin_debug};
use serde_json::{Value, json};
use std::io::BufRead;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const LONG: Duration = Duration::from_secs(30);
const WM_CLOSE: u32 = 0x0010;
const WM_CHAR: u32 = 0x0102;
const WM_COPYDATA: u32 = 0x004A;
const SMTO_ABORTIFHUNG: u32 = 0x0002;
const COPYDATA_TEST: usize = 0x5355_424B;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;

#[repr(C)]
struct CopyData {
    data: usize,
    len: u32,
    ptr: *const u8,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn PostMessageW(hwnd: isize, msg: u32, wp: usize, lp: isize) -> i32;
    fn SendMessageTimeoutW(
        hwnd: isize,
        msg: u32,
        wp: usize,
        lp: isize,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
}

fn endpoint(tag: &str) -> String {
    format!("subak-e2e23-{tag}-{}", std::process::id())
}

/// `subak --json --endpoint <ep> <args>` (debug build, next to the debug window).
fn json(exe: &Path, ep: &str, args: &[&str]) -> (i32, Value) {
    let out = Command::new(exe)
        .args(["--json", "--endpoint", ep])
        .args(args)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("run subak");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{args:?}: not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    (out.status.code().unwrap_or(-1), doc)
}

/// The Terminals the Host lists: (name, state, pane pid).
fn terminals(exe: &Path, ep: &str) -> Vec<(String, String, u64)> {
    let (code, doc) = json(exe, ep, &["ls"]);
    assert_eq!(code, 0, "{doc}");
    doc["data"]["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["name"].as_str().unwrap().to_string(),
                t["state"].as_str().unwrap().to_string(),
                t["windows"][0]["panes"][0]["pid"].as_u64().unwrap_or(0),
            )
        })
        .collect()
}

/// Stops the Host and its Terminals however the scenario ends.
struct Cleanup<'a> {
    exe: &'a Path,
    ep: &'a str,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = json(
            self.exe,
            self.ep,
            &["host", "stop", "--kill-terminals", "--yes"],
        );
    }
}

/// One off-screen test window.
struct Window {
    child: Child,
    hwnd: isize,
    events: mpsc::Receiver<Value>,
}

impl Window {
    fn open(exe: &Path, ep: &str, target: Option<&str>, appdata: &Path) -> Self {
        let mut cmd = Command::new(exe);
        cmd.args([
            "--test-offscreen",
            "--test-max-seconds",
            "180",
            "--endpoint",
            ep,
        ]);
        if let Some(t) = target {
            cmd.arg(t);
        }
        let mut child = cmd
            .env("APPDATA", appdata)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("start subak-gui");
        let (tx, events) = mpsc::channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    let _ = tx.send(v);
                }
            }
        });
        let mut w = Self {
            child,
            hwnd: 0,
            events,
        };
        w.hwnd = w.wait("ready", |_| true)["hwnd"].as_u64().unwrap() as isize;
        w
    }

    fn wait(&self, name: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let until = Instant::now() + LONG;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(v) if v["event"] == name && pred(&v) => return v,
                Ok(_) => {}
                Err(_) => panic!("subak-gui: no `{name}` event"),
            }
        }
    }

    fn type_text(&self, s: &str) {
        for u in s.encode_utf16() {
            // SAFETY: posts a character to this scenario's own off-screen window.
            let posted = unsafe { PostMessageW(self.hwnd, WM_CHAR, usize::from(u), 1) };
            assert_ne!(posted, 0);
        }
    }

    /// A test command (a modified key, a dump); returns the dump that answers it.
    fn cmd(&self, c: &Value) -> Value {
        let body = c.to_string();
        let cds = CopyData {
            data: COPYDATA_TEST,
            len: body.len() as u32,
            ptr: body.as_ptr(),
        };
        let mut r = 0usize;
        // SAFETY: a synchronous WM_COPYDATA to this scenario's own window; `cds` and `body`
        // outlive the call.
        let ok = unsafe {
            SendMessageTimeoutW(
                self.hwnd,
                WM_COPYDATA,
                0,
                (&raw const cds) as isize,
                SMTO_ABORTIFHUNG,
                10_000,
                &raw mut r,
            )
        };
        assert_ne!(ok, 0, "WM_COPYDATA");
        self.wait("dump", |_| true)
    }

    fn key(&self, vk: u32, ctrl: bool, shift: bool) -> Value {
        self.cmd(&json!({"cmd": "key", "vk": vk, "ctrl": ctrl, "shift": shift}))
    }

    /// Dumps until `pred` holds.
    fn until(&self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let until = Instant::now() + LONG;
        loop {
            let d = self.cmd(&json!({"cmd": "dump"}));
            if pred(&d) {
                return d;
            }
            assert!(Instant::now() < until, "{what} did not happen: {d:#}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn close(&mut self) -> i32 {
        // SAFETY: the close message of this scenario's own window (a detach).
        unsafe { PostMessageW(self.hwnd, WM_CLOSE, 0, 0) };
        let until = Instant::now() + LONG;
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                return s.code().unwrap_or(-1);
            }
            assert!(Instant::now() < until, "subak-gui did not exit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tab_names(d: &Value) -> Vec<String> {
    d["tabs"]
        .as_array()
        .map(|t| {
            t.iter()
                .map(|x| x["name"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn windows(exe: &Path, ep: &str, t: &str) -> Vec<Value> {
    let (code, doc) = json(exe, ep, &["window", "ls", t]);
    assert_eq!(code, 0, "{doc}");
    doc["data"]["windows"].as_array().unwrap().clone()
}

fn active_window(exe: &Path, ep: &str, t: &str) -> String {
    let (_, doc) = json(exe, ep, &["window", "ls", t]);
    doc["data"]["active_window"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + LONG;
    while !f() {
        assert!(Instant::now() < until, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn the_prefix_drives_windows_and_panes_of_the_shown_terminal_and_passes_tab_keys_on() {
    let subak = bin_debug("subak");
    let gui = bin_debug("subak-gui");
    let ep = endpoint("prefix");
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
    };
    let appdata = TempDir::new("e2e23-prefix");
    let (code, doc) = json(&subak, &ep, &["new", "other"]);
    assert_eq!(code, 0, "{doc}");
    let (code, doc) = json(&subak, &ep, &["new", "work"]);
    assert_eq!(code, 0, "{doc}");
    let mut w = Window::open(&gui, &ep, Some("work"), appdata.path());
    let d = w.until("work shown", |d| {
        d["phase"] == "attached" && d["terminal"]["name"] == "work"
    });
    let tabs = tab_names(&d);
    let prefix = |w: &Window| w.key(u32::from(b'B'), true, false);
    let typed = |w: &Window, vk: u32, shift: bool, ch: &str| {
        w.cmd(&json!({"cmd": "key", "vk": vk, "shift": shift, "char": ch}))
    };
    // Ctrl+B c: an internal window of `work`, not a tab.
    prefix(&w);
    typed(&w, u32::from(b'C'), false, "c");
    wait_for("two windows", || windows(&subak, &ep, "work").len() == 2);
    let w1 = windows(&subak, &ep, "work")[1]["id"]
        .as_str()
        .unwrap()
        .to_string();
    wait_for("window 1 active", || {
        active_window(&subak, &ep, "work") == w1
    });
    // Ctrl+B p, n, 0.
    prefix(&w);
    typed(&w, u32::from(b'P'), false, "p");
    wait_for("window 0", || active_window(&subak, &ep, "work") != w1);
    prefix(&w);
    typed(&w, u32::from(b'N'), false, "n");
    wait_for("window 1", || active_window(&subak, &ep, "work") == w1);
    prefix(&w);
    w.key(u32::from(b'0'), false, false);
    wait_for("window 0 by number", || {
        active_window(&subak, &ep, "work") != w1
    });
    // Ctrl+B %: a split; Ctrl+B z: zoomed and back.
    prefix(&w);
    typed(&w, 0x35, true, "%");
    w.until("split", |d| {
        d["layout"]["slots"]
            .as_array()
            .is_some_and(|s| s.len() == 2)
    });
    prefix(&w);
    typed(&w, u32::from(b'Z'), false, "z");
    w.until("zoomed", |d| d["layout"].is_null());
    prefix(&w);
    typed(&w, u32::from(b'Z'), false, "z");
    w.until("unzoomed", |d| !d["layout"].is_null());
    // Ctrl+B Ctrl+T types Ctrl+T: no new tab.
    prefix(&w);
    let d = w.key(u32::from(b'T'), true, false);
    assert_eq!(tab_names(&d), tabs, "no tab was opened");
    // F1 shows the keys; Esc closes them.
    w.key(0x70, false, false);
    let d = w.until("help", |d| !d["help"].is_null());
    let screen: Vec<String> = d["screen"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        screen
            .iter()
            .any(|l| l.contains("Ctrl+B c ") && l.contains("New window")),
        "{screen:#?}"
    );
    w.key(VK_ESCAPE, false, false);
    w.until("help closed", |d| d["help"].is_null());
    // Ctrl+B d: the window lets go; every Terminal keeps running.
    prefix(&w);
    typed(&w, u32::from(b'D'), false, "d");
    assert_eq!(
        w.close(),
        0,
        "Ctrl+B d closed the window; WM_CLOSE finds it gone"
    );
    let all = terminals(&subak, &ep);
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|t| t.1 == "running"), "{all:?}");
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn the_console_clients_detach_keys_follow_hotkeys_json() {
    let subak = kim_e2e::bin("subak");
    let ep = endpoint("cli");
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
    };
    let appdata = TempDir::new("e2e23-cli");
    let dir = appdata.path().join("mouv").join("subak");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("hotkeys.json"),
        json!({"schema": "kim.hotkeys", "schema_version": 1, "tool": "subak",
            "leaders": {"prefix": "Ctrl+A"},
            "overrides": [{"action": "client.detach", "bindings": ["@prefix char:q"]}]})
        .to_string(),
    )
    .unwrap();
    let (code, doc) = json(&subak, &ep, &["new", "keys"]);
    assert_eq!(code, 0, "{doc}");
    // Piped input: Ctrl+B d is ordinary input, Ctrl+A q detaches before the rest is sent.
    let mut child = Command::new(&subak)
        .args(["--endpoint", &ep, "attach", "keys"])
        .env("APPDATA", appdata.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .expect("attach");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&[0x02, b'd', 0x0D]).unwrap();
        stdin.write_all(b"echo before-detach-ma^rk\r").unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        stdin.write_all(&[0x01, b'q']).unwrap();
        stdin.write_all(b"echo after-detach-ma^rk\r").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        drop(stdin);
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Ctrl+A q detaches"), "{err}");
    let (_, cap) = json(&subak, &ep, &["capture", "keys"]);
    let text = cap["data"]["lines"].to_string();
    assert!(text.contains("before-detach-mark"), "{text}");
    assert!(
        !text.contains("after-detach-mark"),
        "nothing after the detach keys: {text}"
    );
}

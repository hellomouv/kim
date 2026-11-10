//! E2E — Step 24: tmux command lines, copy mode and paste buffers (ADR 0058).
//! `SUBAK-FR-013`, `SUBAK-FR-017`, `SUBAK-FR-018`.
//!
//! Scenario A (release `subak`): a script drives a Terminal with tmux commands — a chain of
//! splits, a pane broken out into its own window and joined back (its process unchanged),
//! `send-keys` with key names — and with paste buffers (set, list with a redacted preview, paste as
//! input); a kill without `--yes` and a relative tmux target are refused.
//!
//! Scenario B (debug `subak-gui`, off screen): the command prompt splits the shown Terminal and
//! shows `list-panes`; copy mode copies an output line into a buffer that `Ctrl+B ]` pastes.
//!
//! Windows only; private endpoints and `APPDATA` sandboxes; everything started is stopped.
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
    format!("subak-e2e24-{tag}-{}", std::process::id())
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

fn capture(exe: &Path, ep: &str, t: &str) -> String {
    let (_, doc) = json(exe, ep, &["capture", t]);
    doc["data"]["lines"].to_string()
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + LONG;
    while !f() {
        assert!(Instant::now() < until, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn ok(exe: &Path, ep: &str, args: &[&str]) -> Value {
    let (code, doc) = json(exe, ep, args);
    assert_eq!(code, 0, "{args:?}: {doc}");
    doc["data"].clone()
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_script_drives_a_terminal_with_tmux_commands_and_paste_buffers() {
    let subak = kim_e2e::bin("subak");
    let ep = endpoint("cli");
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
    };
    ok(&subak, &ep, &["new", "work", "--size", "100x30"]);
    let d = ok(
        &subak,
        &ep,
        &[
            "command",
            "--target",
            "work",
            "split-window -h; split-window -v; list-panes",
        ],
    );
    assert_eq!(d["results"].as_array().unwrap().len(), 3);
    let moved = d["results"][0]["result"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    let pid = |exe: &Path, ep: &str| -> Value {
        let d = ok(exe, ep, &["pane", "ls", "work"]);
        d["windows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|w| w["panes"].as_array().unwrap().clone())
            .find(|p| p["id"] == moved.as_str())
            .unwrap()["pid"]
            .clone()
    };
    let before = pid(&subak, &ep);
    ok(
        &subak,
        &ep,
        &["command", "--target", &moved, "break-pane -n solo"],
    );
    assert_eq!(
        ok(&subak, &ep, &["window", "ls", "work"])["windows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    ok(
        &subak,
        &ep,
        &[
            "command",
            "--",
            "join-pane",
            "-h",
            "-s",
            &moved,
            "-t",
            "work:0.0",
        ],
    );
    assert_eq!(
        ok(&subak, &ep, &["window", "ls", "work"])["windows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        pid(&subak, &ep),
        before,
        "the same process moved out and back"
    );
    ok(
        &subak,
        &ep,
        &[
            "command",
            "--target",
            "work:0.0",
            "send-keys 'echo e2e-se^nt' Enter",
        ],
    );
    wait_for("send-keys", || {
        capture(&subak, &ep, "work:0.0").contains("e2e-sent")
    });
    ok(&subak, &ep, &["buffer", "set", "echo e2e-buff^er\n"]);
    ok(
        &subak,
        &ep,
        &[
            "buffer",
            "set",
            "--name",
            "secret",
            "token ghp_abcdefghijklmnopqrstuvwxyz0123456789AB",
        ],
    );
    let d = ok(&subak, &ep, &["buffer", "ls"]);
    assert!(
        !d.to_string().contains("ghp_abc"),
        "previews are redacted: {d}"
    );
    ok(
        &subak,
        &ep,
        &["buffer", "paste", "work:0.0", "--name", "buffer0"],
    );
    wait_for("paste", || {
        capture(&subak, &ep, "work:0.0").contains("e2e-buffer")
    });
    let (code, doc) = json(&subak, &ep, &["command", "--target", "work", "kill-pane"]);
    assert_eq!(
        (code, doc["error"]["code"].as_str()),
        (7, Some("confirmation_required"))
    );
    let (code, doc) = json(&subak, &ep, &["command", "select-pane -t {last}"]);
    assert_eq!((code, doc["error"]["code"].as_str()), (2, Some("usage")));
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn the_window_runs_prompt_commands_and_copies_with_copy_mode() {
    let subak = bin_debug("subak");
    let gui = bin_debug("subak-gui");
    let ep = endpoint("gui");
    let _c = Cleanup {
        exe: &subak,
        ep: &ep,
    };
    let appdata = TempDir::new("e2e24-gui");
    let (code, doc) = json(&subak, &ep, &["new", "work"]);
    assert_eq!(code, 0, "{doc}");
    let w = Window::open(&gui, &ep, Some("work"), appdata.path());
    let d = w.until("attached", |d| d["phase"] == "attached");
    let pane0 = d["pane_id"].as_str().unwrap().to_string();
    w.type_text("echo copy-this-li^ne\r");
    wait_for("output", || {
        capture(&subak, &ep, &pane0).contains("copy-this-line")
    });
    let prefix = |w: &Window| w.key(u32::from(b'B'), true, false);
    let typed = |w: &Window, vk: u32, shift: bool, ch: &str| {
        w.cmd(&json!({"cmd": "key", "vk": vk, "shift": shift, "char": ch}))
    };
    prefix(&w);
    typed(&w, 0xBA, true, ":");
    w.until("prompt", |d| d["prompt"] == "");
    w.cmd(&json!({"cmd": "type", "text": "split-window -h; select-pane -L; list-panes"}));
    w.key(0x0D, false, false);
    let d = w.until("answer", |d| !d["help"].is_null());
    assert!(d["screen"].to_string().contains("PANE"), "{}", d["screen"]);
    w.key(0x1B, false, false);
    // Copy mode: the output line into a buffer, pasted with Ctrl+B ].
    prefix(&w);
    typed(&w, 0xDB, false, "[");
    let d = w.until("copy mode", |d| d["copy_mode"] == true);
    let row = d["screen"]
        .as_array()
        .unwrap()
        .iter()
        .rposition(|l| l.as_str().unwrap().trim() == "copy-this-line")
        .unwrap();
    let cur = d["view_cursor"][1].as_u64().unwrap() as usize;
    for _ in row..cur {
        typed(&w, u32::from(b'K'), false, "k");
    }
    typed(&w, u32::from(b'0'), false, "0");
    w.key(0x20, false, false);
    typed(&w, u32::from(b'4'), true, "$");
    w.key(0x0D, false, false);
    w.until("copied", |d| d["copy_mode"] == false);
    let (_, doc) = json(&subak, &ep, &["buffer", "show"]);
    assert_eq!(
        doc["data"]["data"].as_str().map(str::trim),
        Some("copy-this-line")
    );
    prefix(&w);
    typed(&w, 0xDD, false, "]");
    wait_for("pasted", || {
        capture(&subak, &ep, &pane0)
            .matches("copy-this-line")
            .count()
            >= 2
    });
}

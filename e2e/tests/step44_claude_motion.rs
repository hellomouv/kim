//! E2E — Step 44: Subak's watermelon icon and each Terminal's moving Claude mark (ADR 0073).
//! `SUBAK-FR-054`, `SUBAK-FR-051`.
//!
//! A user with two Terminals: Claude starts working in one (a **stand-in**: the installed
//! reporter fed hook JSON from inside the pane, not a Claude process), asks for permission in
//! the other. The window's tab bar turns the working Terminal's mark and blinks the other's,
//! each by its own state; minimizing stops every frame; `subak config set
//! terminal.claude_animation false` makes the marks static in the next window; the CLI reports
//! the same states. The shipped icon assets exist and the `.ico` is a valid multi-size icon.
//!
//! The window is the debug build's off-screen test window (`--test-offscreen`); a private
//! endpoint and `APPDATA` sandbox; everything started is stopped. Windows only.
#![cfg(windows)]
#![allow(unsafe_code)]

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
const WM_COPYDATA: u32 = 0x004A;
const SMTO_ABORTIFHUNG: u32 = 0x0002;
const COPYDATA_TEST: usize = 0x5355_424B;

#[repr(C)]
struct CopyData {
    data: usize,
    len: u32,
    ptr: *const u8,
}

#[link(name = "user32")]
unsafe extern "system" {
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

fn json(exe: &Path, appdata: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(exe)
        .arg("--json")
        .args(args)
        .env("APPDATA", appdata)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("run subak");
    let doc = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap_or(-1), doc)
}

struct Window {
    child: Child,
    hwnd: isize,
    events: mpsc::Receiver<Value>,
}

impl Window {
    fn open(exe: &Path, ep: &str, target: &str, appdata: &Path) -> Self {
        let mut child = Command::new(exe)
            .args([
                "--test-offscreen",
                "--test-max-seconds",
                "120",
                "--endpoint",
                ep,
                target,
            ])
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
        w.hwnd = w.wait("ready")["hwnd"].as_u64().unwrap() as isize;
        w
    }

    fn wait(&self, name: &str) -> Value {
        let until = Instant::now() + LONG;
        loop {
            match self
                .events
                .recv_timeout(until.saturating_duration_since(Instant::now()))
            {
                Ok(v) if v["event"] == name => return v,
                Ok(_) => {}
                Err(_) => panic!("subak-gui: no `{name}` event"),
            }
        }
    }

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
        self.wait("dump")
    }

    fn until(&self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let until = Instant::now() + LONG;
        loop {
            let d = self.cmd(&json!({"cmd": "dump"}));
            if pred(&d) {
                return d;
            }
            assert!(Instant::now() < until, "{what}: {d:#}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tab(d: &Value, name: &str) -> String {
    d["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .map_or(String::new(), |t| {
            t["claude"].as_str().unwrap_or("").to_string()
        })
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn each_terminal_mark_moves_by_its_own_claude_state_only_while_shown() {
    let (subak, gui) = (bin_debug("subak"), bin_debug("subak-gui"));
    let tmp = TempDir::new("e2e44");
    let appdata = tmp.path().join("appdata");
    std::fs::create_dir_all(&appdata).unwrap();
    let ep = format!("subak-e2e44-{}", std::process::id());
    struct Stop<'a>(&'a Path, &'a Path, String);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            let _ = json(
                self.0,
                self.1,
                &[
                    "--endpoint",
                    &self.2,
                    "host",
                    "stop",
                    "--kill-terminals",
                    "--yes",
                ],
            );
        }
    }
    let _stop = Stop(&subak, &appdata, ep.clone());
    for name in ["agent", "asker"] {
        let (code, doc) = json(&subak, &appdata, &["--endpoint", &ep, "new", name]);
        assert_eq!(code, 0, "{doc}");
    }
    let mut k = 0;
    let mut hook = |target: &str, input: Value| {
        k += 1;
        let (file, out) = (
            tmp.path().join(format!("{k}.json")),
            tmp.path().join(format!("{k}.out")),
        );
        std::fs::write(&file, input.to_string()).unwrap();
        let line = format!(
            "type \"{}\" | \"{}\" --json claude report > \"{}\"",
            file.display(),
            subak.display(),
            out.display()
        );
        for keys in [
            format!("send-keys -l '{line}'"),
            "send-keys Enter".to_string(),
        ] {
            let (code, doc) = json(
                &subak,
                &appdata,
                &["--endpoint", &ep, "command", "--target", target, &keys],
            );
            assert_eq!(code, 0, "{doc}");
        }
        let until = Instant::now() + LONG;
        while !std::fs::read_to_string(&out).is_ok_and(|t| t.contains("\"delivered\":true")) {
            assert!(Instant::now() < until, "the reporter");
            std::thread::sleep(Duration::from_millis(100));
        }
    };

    let w = Window::open(&gui, &ep, "agent", &appdata);
    hook(
        "agent",
        json!({"session_id": "e1", "hook_event_name": "UserPromptSubmit", "prompt": "p"}),
    );
    hook(
        "asker",
        json!({"session_id": "e2", "hook_event_name": "PermissionRequest", "tool_name": "Bash"}),
    );
    let d = w.until("both states", |d| {
        tab(d, "agent") == "in-progress" && tab(d, "asker") == "need-input"
    });
    assert!(
        d["claude_motion"]["timer_ms"].is_u64(),
        "{}",
        d["claude_motion"]
    );
    // The working mark turns: several spin glyphs within 1.5 s.
    let mut seen = std::collections::BTreeSet::new();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(1_500) {
        let text = w.cmd(&json!({"cmd": "dump"}))["bar"]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        seen.extend(text.chars().filter(|c| ['◐', '◓', '◑', '◒'].contains(c)));
        std::thread::sleep(Duration::from_millis(60));
    }
    assert!(seen.len() >= 3, "{seen:?}");
    // Minimized: no timer, no frames.
    let d = w.cmd(&json!({"cmd": "minimized", "on": true}));
    assert!(
        d["claude_motion"]["timer_ms"].is_null(),
        "{}",
        d["claude_motion"]
    );
    // The CLI says the same states.
    let (_, doc) = json(&subak, &appdata, &["--endpoint", &ep, "ls"]);
    for t in doc["data"]["terminals"].as_array().unwrap() {
        assert_eq!(
            t["claude_state"],
            tab(&d, t["name"].as_str().unwrap()),
            "{t}"
        );
    }
    drop(w);

    // Static marks by setting: no timer while Claude works.
    let (code, doc) = json(
        &subak,
        &appdata,
        &["config", "set", "terminal.claude_animation", "false"],
    );
    assert_eq!(code, 0, "{doc}");
    let w = Window::open(&gui, &ep, "agent", &appdata);
    let d = w.until("states", |d| tab(d, "agent") == "in-progress");
    assert_eq!(
        d["claude_motion"]["mode"], "static",
        "{}",
        d["claude_motion"]
    );
    assert!(d["claude_motion"]["timer_ms"].is_null());
    assert!(
        d["bar"]["text"].as_str().unwrap().contains('◔'),
        "{}",
        d["bar"]["text"]
    );
    drop(w);

    // The shipped icon: a valid multi-size .ico and the PNG and SVG sources.
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/subak");
    let ico = std::fs::read(assets.join("subak.ico")).unwrap();
    assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    assert!(u16::from_le_bytes([ico[4], ico[5]]) >= 5, "several sizes");
    for f in [
        "subak.svg",
        "subak-16.png",
        "subak-32.png",
        "subak-48.png",
        "subak-mono-dark-16.png",
    ] {
        assert!(assets.join(f).is_file(), "{f}");
    }
}

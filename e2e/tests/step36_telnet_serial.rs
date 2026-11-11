//! E2E — Step 36: Telnet and serial panes (ADR 0069). `SUBAK-FR-061`, `KIM-SEC-013`.
//!
//! With the release `subak`: Telnet is refused until the user opts in, then a pane talks to a
//! local Telnet server (negotiation, a typed line, the server's answer, the hang-up); a serial
//! pane on a port that does not exist says so while the Host keeps serving.
#![cfg(windows)]

use serde_json::Value;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NO_WINDOW: u32 = 0x0800_0000;

struct Env {
    ep: String,
    appdata: std::path::PathBuf,
}

impl Env {
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let local = args.starts_with(&["serial", "list"]);
        let mut c = Command::new(kim_e2e::bin("subak"));
        c.arg("--json");
        if !local {
            c.args(["--endpoint", &self.ep]);
        }
        let out = c
            .args(args)
            .env("APPDATA", &self.appdata)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(NO_WINDOW)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            serde_json::from_slice(&out.stdout).unwrap(),
        )
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.run(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn wait(&self, t: &str, want: &str) -> String {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let (_, d) = self.run(&["capture", t]);
            let s = d["data"]["lines"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if s.contains(want) {
                return s;
            }
            assert!(Instant::now() < end, "{t} never showed {want:?}:\n{s}");
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.run(&["host", "stop", "--kill-terminals", "--yes"]);
        let _ = std::fs::remove_dir_all(&self.appdata);
    }
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_user_opts_into_telnet_and_sees_a_missing_serial_port_reported() {
    let id = std::process::id();
    let appdata = std::env::temp_dir().join(format!("subak-e2e36-{id}"));
    std::fs::create_dir_all(&appdata).unwrap();
    let env = Env {
        ep: format!("subak-e2e36-{id}"),
        appdata,
    };
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        // DO TTYPE, DO NAWS, WILL ECHO, then a prompt.
        c.write_all(&[255, 253, 24, 255, 253, 31, 255, 251, 1])
            .unwrap();
        c.write_all(b"Router login: ").unwrap();
        let mut seen = Vec::new();
        let mut buf = [0u8; 512];
        while let Ok(n) = c.read(&mut buf) {
            if n == 0 {
                return;
            }
            seen.extend_from_slice(&buf[..n]);
            if seen.windows(7).any(|w| w == b"admin\r\n") {
                c.write_all(b"\r\nWelcome, admin\r\n").unwrap();
                return;
            }
        }
    });
    let dest = format!("127.0.0.1:{port}");
    let (code, doc) = env.run(&["telnet", "connect", &dest, "router"]);
    assert_eq!(code, 7, "{doc}");
    let d = env.ok(&["telnet", "connect", &dest, "router", "--allow-insecure"]);
    assert!(
        d["telnet"]["warnings"][0]
            .as_str()
            .unwrap()
            .contains("unencrypted")
    );
    env.wait("router", "Router login:");
    env.ok(&["command", "--target", "router", "send-keys -l 'admin'"]);
    env.ok(&["command", "--target", "router", "send-keys Enter"]);
    env.wait("router", "Welcome, admin");
    env.wait("router", "Telnet connection");

    assert!(env.ok(&["serial", "list"])["ports"].is_array());
    env.ok(&["serial", "connect", "COM251", "dev", "--baud", "57600"]);
    env.wait("dev", "cannot open COM251");
    env.ok(&["host", "health"]);
}

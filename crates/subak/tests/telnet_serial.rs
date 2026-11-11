//! Step 36 — Telnet and serial panes, ssh config hosts (ADR 0069). `SUBAK-FR-061`,
//! `KIM-SEC-013`.
//!
//! A real Host and the real `subak.exe` bridge in a pane against an in-process Telnet server:
//! refused without `--allow-insecure`, then the negotiation (terminal type, window size, echo),
//! a login exchange typed through the pane, the server hanging up (the pane keeps its last
//! screen). Serial: the port list, settings checked, a port that does not exist reported in
//! the pane without hurting the Host. No serial hardware is attached to the development PC.
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::Value;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IAC: u8 = 255;

struct Lab {
    ep: String,
}

impl Lab {
    fn cli(&self, args: &[&str]) -> (i32, Value) {
        let local = matches!(args.first(), Some(&("serial" | "ssh")))
            && !args.starts_with(&["serial", "connect"]);
        let mut a = Vec::new();
        if !local {
            a.extend(["--endpoint", self.ep.as_str()]);
        }
        a.extend_from_slice(args);
        let (code, doc) = run(&a);
        validate("subak-cli-v1", &doc);
        (code, doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn screen(&self, t: &str) -> String {
        let (code, d) = self.cli(&["capture", t]);
        if code != 0 {
            return String::new();
        }
        d["data"]["lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait(&self, t: &str, want: &str) -> String {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let s = self.screen(t);
            if s.contains(want) {
                return s;
            }
            assert!(Instant::now() < end, "{t} never showed {want:?}:\n{s}");
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    fn keys(&self, t: &str, text: &str) {
        self.ok(&["command", "--target", t, &format!("send-keys -l '{text}'")]);
        self.ok(&["command", "--target", t, "send-keys Enter"]);
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = run(&[
            "host",
            "stop",
            "--endpoint",
            &self.ep,
            "--kill-terminals",
            "--yes",
        ]);
    }
}

/// Data bytes with Telnet commands removed (what the user typed).
fn data_only(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == IAC && i + 1 < b.len() {
            match b[i + 1] {
                IAC => {
                    out.push(IAC);
                    i += 2;
                }
                250 => {
                    while i + 1 < b.len() && !(b[i] == IAC && b[i + 1] == 240) {
                        i += 1;
                    }
                    i += 2;
                }
                251..=254 => i += 3,
                _ => i += 2,
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn telnet_is_opt_in_negotiates_safely_and_serial_reports_a_missing_port() {
    let lab = Lab {
        ep: endpoint(&format!("w36-{}", std::process::id())),
    };
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let got = Arc::new(Mutex::new(Vec::<u8>::new()));
    let g = got.clone();
    std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let mut hello = vec![IAC, 253, 24, IAC, 253, 31, IAC, 251, 1, IAC, 251, 3];
        hello.extend([IAC, 250, 24, 1, IAC, 240]);
        hello.extend(b"Welcome to the fake router\r\nlogin: ");
        c.write_all(&hello).unwrap();
        let mut buf = [0u8; 1024];
        let mut greeted = false;
        loop {
            let n = match c.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let all = {
                let mut v = g.lock().unwrap();
                v.extend_from_slice(&buf[..n]);
                v.clone()
            };
            let d = data_only(&all);
            if !greeted && contains(&d, b"guest\r\n") {
                greeted = true;
                c.write_all(b"\r\nhello guest\r\nrouter> ").unwrap();
            }
            if contains(&d, b"quit\r\n") {
                c.write_all(b"\r\nbye\r\n").unwrap();
                return; // hangs up
            }
        }
    });
    let dest = format!("127.0.0.1:{port}");

    // Off by default: nothing starts without --allow-insecure.
    let (code, doc) = lab.cli(&["telnet", "connect", &dest, "router"]);
    assert_eq!(code, 7, "{doc}");
    assert_eq!(doc["error"]["code"], "invalid_state");
    assert!(
        doc["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unencrypted")
    );
    let (_, doc) = lab.cli(&["host", "status"]);
    assert_eq!(
        doc["error"]["code"], "host_not_running",
        "no Host was started for a refused connection: {doc}"
    );

    let d = lab.ok(&["telnet", "connect", &dest, "router", "--allow-insecure"]);
    assert!(
        d["telnet"]["warnings"][0]
            .as_str()
            .unwrap()
            .contains("unencrypted"),
        "{d}"
    );
    lab.wait("router", "login:");
    lab.keys("router", "guest");
    lab.wait("router", "hello guest");
    let wire = got.lock().unwrap().clone();
    for (what, seq) in [
        ("WILL TTYPE", vec![IAC, 251, 24]),
        ("WILL NAWS", vec![IAC, 251, 31]),
        ("DO ECHO", vec![IAC, 253, 1]),
        ("DO SGA", vec![IAC, 253, 3]),
        ("NAWS size", vec![IAC, 250, 31]),
    ] {
        assert!(contains(&wire, &seq), "{what} missing: {wire:?}");
    }
    assert!(contains(&wire, b"XTERM-256COLOR"));
    lab.keys("router", "quit");
    let s = lab.wait("router", "Telnet connection");
    assert!(s.contains("bye"), "{s}");
    let info = lab.ok(&["info", "router"]);
    assert_ne!(
        info["state"], "running",
        "the bridge ended with the connection: {info}"
    );

    // Serial: the list, the checks, and a port that is not there.
    let ports = lab.ok(&["serial", "list"]);
    assert!(ports["ports"].is_array());
    let (code, doc) = lab.cli(&["serial", "connect", "LPT1"]);
    assert_eq!(
        (code, doc["error"]["code"].as_str()),
        (2, Some("usage")),
        "{doc}"
    );
    let (code, _) = lab.cli(&["serial", "connect", "COM3", "--parity", "sometimes"]);
    assert_eq!(code, 2);
    let d = lab.ok(&[
        "serial",
        "connect",
        "COM250",
        "nocom",
        "--baud",
        "115200",
        "--flow",
        "rtscts",
        "--dry-run",
    ]);
    assert_eq!(d["serial"]["baud"], 115_200, "{d}");
    let d = lab.ok(&["serial", "connect", "COM250", "nocom", "--baud", "115200"]);
    assert_eq!(d["serial"]["flow"], "none");
    lab.wait("nocom", "cannot open COM250");
    // The Host keeps serving.
    lab.ok(&["host", "health"]);

    // The hosts of an ssh config.
    let dir = std::env::temp_dir().join(format!("subak-t36-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("config");
    std::fs::write(
        &cfg,
        "Host lab\n  HostName 10.1.2.3\n  User me\nHost *\n  User x\n",
    )
    .unwrap();
    let h = lab.ok(&["ssh", "hosts", "--ssh-config", &cfg.display().to_string()]);
    assert_eq!(h["hosts"].as_array().unwrap().len(), 1, "{h}");
    assert_eq!(h["hosts"][0]["hostname"], "10.1.2.3");
    let _ = std::fs::remove_dir_all(&dir);
}

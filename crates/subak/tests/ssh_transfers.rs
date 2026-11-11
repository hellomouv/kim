//! Step 35 — SFTP transfers, jump hosts, port forwarding, proxies and login scripts (ADR 0068).
//! `SUBAK-FR-061`, `KIM-SEC-013`, `KIM-FR-013`.
//!
//! Two real SSH servers (Microsoft's portable sshd from `scripts/fetch-test-sshd.ps1`, run as
//! this user on 127.0.0.1 with SFTP) and Windows' real OpenSSH client. Scenarios: an unknown host
//! refused, listing, a folder tree up and down, a 64 MiB file byte for byte, overwrite
//! protection, a cancelled upload resumed, a server killed mid-download and resumed, delete
//! confirmation, one and two jump hosts, local/remote/dynamic forwards, a taken port, SOCKS5
//! and HTTP proxies, agent and X11 forwarding guarded, a login script, refused options.
//! Without the server the test says so and does nothing (it never downloads).
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::Value;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NO_WINDOW: u32 = 0x0800_0000;

fn sshd_dir() -> Option<PathBuf> {
    let d = std::env::var_os("SUBAK_TEST_SSHD_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-sshd/OpenSSH-Win64"),
        PathBuf::from,
    );
    d.join("sshd.exe").is_file().then_some(d)
}

fn fwd(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Server {
    exe: PathBuf,
    config: PathBuf,
    port: u16,
    pid: Option<u32>,
}

impl Server {
    /// On the private desktop: the session programs it starts (`cmd.exe /c sftp-server.exe`)
    /// get console windows, which must never reach the user's screen.
    fn start(&mut self) {
        self.pid = Some(hidden::spawn(
            &self.exe,
            &["-D".into(), "-f".into(), self.config.display().to_string()],
        ));
        let end = Instant::now() + Duration::from_secs(10);
        while std::net::TcpStream::connect(("127.0.0.1", self.port)).is_err() {
            assert!(Instant::now() < end, "sshd on {} did not start", self.port);
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Ends the server and its live sessions (a dropped connection for every client).
    fn stop(&mut self) {
        if let Some(pid) = self.pid.take() {
            hidden::kill_tree(pid);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Lab {
    ep: String,
    appdata: PathBuf,
    dir: PathBuf,
    env: Vec<(String, String)>,
}

impl Lab {
    fn cli_env(&self, args: &[&str], extra: &[(&str, &str)]) -> (i32, Value) {
        let local = matches!(args.first(), Some(&("sftp" | "transfer")));
        let mut a = Vec::new();
        if !local {
            a.extend(["--endpoint", self.ep.as_str()]);
        }
        a.extend_from_slice(args);
        let mut env: Vec<(&str, &str)> = vec![("APPDATA", self.appdata.to_str().unwrap())];
        env.extend(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        env.extend_from_slice(extra);
        let (code, doc) = run_env(&a, &env);
        validate("subak-cli-v1", &doc);
        (code, doc)
    }

    fn cli(&self, args: &[&str]) -> (i32, Value) {
        self.cli_env(args, &[])
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn fails(&self, args: &[&str], code: &str) -> Value {
        let (c, doc) = self.cli(args);
        assert_ne!(c, 0, "{args:?}: {doc}");
        assert_eq!(doc["error"]["code"], code, "{args:?}: {doc}");
        doc["error"].clone()
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

    fn job(&self, id: &str) -> Value {
        self.ok(&["transfer", "status", id])
    }

    /// Waits until job `id` satisfies `f`.
    fn job_until(&self, id: &str, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let j = self.job(id);
            if f(&j) {
                return j;
            }
            assert!(Instant::now() < end, "transfer {id} never {what}: {j}");
            std::thread::sleep(Duration::from_millis(50));
        }
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
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn keygen(bin: &Path, file: &Path) {
    let st = Command::new(bin.join("ssh-keygen.exe"))
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .creation_flags(NO_WINDOW)
        .status()
        .unwrap();
    assert!(st.success());
}

/// Deterministic, incompressible-enough content.
fn pattern(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x & 0xff) as u8
        })
        .collect()
}

fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::read(a).unwrap() == std::fs::read(b).unwrap()
}

/// A minimal HTTP CONNECT proxy on loopback (one connection at a time).
fn http_proxy() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut c = c;
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if c.read_exact(&mut b).is_err() {
                        return;
                    }
                    head.push(b[0]);
                }
                let line = String::from_utf8_lossy(&head).to_string();
                let to = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let Ok(up) = std::net::TcpStream::connect(&to) else {
                    let _ = c.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n");
                    return;
                };
                let _ = c.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                let (mut c2, mut up2) = (c.try_clone().unwrap(), up.try_clone().unwrap());
                let mut up = up;
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut c2, &mut up);
                    let _ = up.shutdown(std::net::Shutdown::Write);
                });
                let _ = std::io::copy(&mut up2, &mut c);
            });
        }
    });
    port
}

#[test]
fn sftp_forwarding_and_jump_hosts_work_against_real_servers() {
    let Some(bin) = sshd_dir() else {
        eprintln!(
            "NOT TESTED: no test sshd (run `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-test-sshd.ps1`)"
        );
        return;
    };
    let id = std::process::id();
    let dir = std::env::temp_dir().join(format!("subak-ssh35-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    let appdata = dir.join("appdata");
    std::fs::create_dir_all(appdata.join("mouv").join("subak")).unwrap();
    std::fs::create_dir_all(dir.join("remote")).unwrap();
    std::fs::create_dir_all(dir.join("local")).unwrap();
    let lab = Lab {
        ep: endpoint(&format!("w35-{id}")),
        appdata,
        dir: dir.clone(),
        env: Vec::new(),
    };
    let host_key = dir.join("host_ed25519");
    let key = dir.join("user_key");
    keygen(&bin, &host_key);
    keygen(&bin, &key);
    std::fs::copy(key.with_extension("pub"), dir.join("authorized_keys")).unwrap();
    let host_pub = std::fs::read_to_string(host_key.with_extension("pub")).unwrap();
    let host_pub: String = host_pub
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let server = |name: &str| {
        let port = free_port();
        let cfg = dir.join(format!("sshd_{name}"));
        std::fs::write(
            &cfg,
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nStrictModes no\nPidFile {}\nAllowTcpForwarding yes\nSubsystem sftp {}\n",
                fwd(&host_key),
                fwd(&dir.join("authorized_keys")),
                fwd(&dir.join(format!("{name}.pid"))),
                fwd(&bin.join("sftp-server.exe")),
            ),
        )
        .unwrap();
        let mut s = Server {
            exe: bin.join("sshd.exe"),
            config: cfg,
            port,
            pid: None,
        };
        s.start();
        s
    };
    let mut a = server("a");
    let b = server("b");
    let c3 = server("c");
    let (lp, rp, dp) = (free_port(), free_port(), free_port());
    // known_hosts: both servers and the ports forwards will expose them on.
    let kh = dir.join("known_hosts");
    let names: String = [a.port, b.port, c3.port, lp, rp]
        .iter()
        .map(|p| format!("[127.0.0.1]:{p} {host_pub}\n"))
        .collect();
    std::fs::write(&kh, names).unwrap();
    let cfg = dir.join("ssh_config");
    std::fs::write(
        &cfg,
        format!(
            "Host *\n  IdentityFile {}\n  IdentitiesOnly yes\n  UserKnownHostsFile {}\n",
            fwd(&key),
            fwd(&kh)
        ),
    )
    .unwrap();
    let cfg_s = cfg.display().to_string();
    let user = std::env::var("USERNAME").unwrap().to_lowercase();
    let at = |p: u16| format!("{user}@127.0.0.1:{p}");
    let (ta, tb) = (at(a.port), at(b.port));
    let remote = fwd(&dir.join("remote"));
    let local = dir.join("local");
    let local_s = local.display().to_string();
    let c = ["--ssh-config", cfg_s.as_str()];
    let with = |base: &[&str]| -> Vec<String> {
        base.iter()
            .chain(c.iter())
            .map(|s| (*s).to_string())
            .collect()
    };
    let call = |v: Vec<String>| -> Value {
        let r: Vec<&str> = v.iter().map(String::as_str).collect();
        lab.ok(&r)
    };

    // An unknown host is refused: transfers never trust a host by themselves.
    let empty_kh = dir.join("empty_known_hosts");
    std::fs::write(&empty_kh, "").unwrap();
    let e = lab.fails(
        &[
            "sftp",
            "ls",
            &ta,
            "--ssh-config",
            &cfg_s,
            "--option",
            &format!("UserKnownHostsFile={}", fwd(&empty_kh)),
        ],
        "ssh_failed",
    );
    assert!(e["hint"].as_str().unwrap().contains("ssh connect"), "{e}");

    // A folder tree up and back down.
    let tree = dir.join("tree");
    std::fs::create_dir_all(tree.join("sub").join("deeper")).unwrap();
    std::fs::write(tree.join("a.txt"), b"alpha").unwrap();
    std::fs::write(tree.join("sub").join("한글.txt"), "가나다").unwrap();
    std::fs::write(
        tree.join("sub").join("deeper").join("z.bin"),
        pattern(100_000, 7),
    )
    .unwrap();
    let d = call(with(&[
        "sftp",
        "put",
        &ta,
        &tree.display().to_string(),
        &remote,
        "--recursive",
    ]));
    assert_eq!(d["result"]["files"], 3, "{d}");
    let ls = call(with(&["sftp", "ls", &ta, &format!("{remote}/tree")]));
    let names: Vec<&str> = ls["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str())
        .collect();
    assert_eq!(names, ["a.txt", "sub"], "{ls}");
    let d = call(with(&[
        "sftp",
        "get",
        &ta,
        &format!("{remote}/tree"),
        &local_s,
        "--recursive",
    ]));
    assert_eq!(d["result"]["files"], 3, "{d}");
    for f in ["a.txt", "sub/한글.txt", "sub/deeper/z.bin"] {
        assert!(same_file(&tree.join(f), &local.join("tree").join(f)), "{f}");
    }

    // A large file, byte for byte, both ways.
    let big = dir.join("big.bin");
    std::fs::write(&big, pattern(64 * 1024 * 1024, 11)).unwrap();
    let d = call(with(&[
        "sftp",
        "put",
        &ta,
        &big.display().to_string(),
        &format!("{remote}/big.bin"),
    ]));
    assert_eq!(d["result"]["bytes"], 64 * 1024 * 1024);
    assert!(same_file(&big, &dir.join("remote").join("big.bin")));
    let back = local.join("big.bin");
    call(with(&[
        "sftp",
        "get",
        &ta,
        &format!("{remote}/big.bin"),
        &back.display().to_string(),
    ]));
    assert!(same_file(&big, &back));

    // An existing destination is kept unless asked.
    let v = with(&[
        "sftp",
        "get",
        &ta,
        &format!("{remote}/big.bin"),
        &back.display().to_string(),
    ]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    let e = lab.fails(&r, "file_exists");
    assert!(e["hint"].as_str().unwrap().contains("--overwrite"));
    let v = with(&[
        "sftp",
        "put",
        &ta,
        &big.display().to_string(),
        &format!("{remote}/big.bin"),
    ]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    lab.fails(&r, "file_exists");
    call(with(&[
        "sftp",
        "get",
        &ta,
        &format!("{remote}/tree/a.txt"),
        &back.display().to_string(),
        "--overwrite",
    ]));
    assert_eq!(std::fs::read(&back).unwrap(), b"alpha");

    // A cancelled background upload keeps its partial file; --resume finishes it.
    let mid = dir.join("mid.bin");
    std::fs::write(&mid, pattern(8 * 1024 * 1024, 23)).unwrap();
    let up_to = format!("{remote}/mid.bin");
    let v = with(&[
        "sftp",
        "put",
        &ta,
        &mid.display().to_string(),
        &up_to,
        "--background",
    ]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    let (code, doc) = lab.cli_env(&r, &[("SUBAK_TEST_SFTP_DELAY_MS", "15")]);
    assert_eq!(code, 0, "{doc}");
    let job = doc["data"]["id"].as_str().unwrap().to_string();
    lab.job_until(&job, "started sending", |j| {
        j["progress"]["bytes_done"].as_u64().unwrap_or(0) > 0
    });
    let cancel = lab.ok(&["transfer", "cancel", &job]);
    assert_eq!(cancel["cancelled"], true, "{cancel}");
    let j = lab.job(&job);
    assert_eq!(j["state"], "cancelled", "{j}");
    let part = dir.join("remote").join("mid.bin.subak-part");
    let had = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    assert!(had > 0 && had < 8 * 1024 * 1024, "partial upload: {had}");
    assert!(!dir.join("remote").join("mid.bin").exists());
    let list = lab.ok(&["transfer", "list"]);
    assert!(
        list["transfers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == job.as_str())
    );
    let d = call(with(&[
        "sftp",
        "put",
        &ta,
        &mid.display().to_string(),
        &up_to,
        "--resume",
    ]));
    assert!(d["result"]["resumed_bytes"].as_u64().unwrap() > 0, "{d}");
    assert!(same_file(&mid, &dir.join("remote").join("mid.bin")));
    assert!(!part.exists());

    // The server dies in the middle of a background download: the job fails with ssh's
    // words, the partial file stays, and --resume completes it once the server is back.
    let down = local.join("mid-down.bin");
    let v = with(&[
        "sftp",
        "get",
        &ta,
        &up_to,
        &down.display().to_string(),
        "--background",
    ]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    let (code, doc) = lab.cli_env(&r, &[("SUBAK_TEST_SFTP_DELAY_MS", "15")]);
    assert_eq!(code, 0, "{doc}");
    let job = doc["data"]["id"].as_str().unwrap().to_string();
    lab.job_until(&job, "started receiving", |j| {
        j["progress"]["bytes_done"].as_u64().unwrap_or(0) > 0
    });
    a.stop();
    let j = lab.job_until(&job, "failed", |j| {
        j["state"] != "running" && j["state"] != "queued"
    });
    assert_eq!(j["state"], "failed", "{j}");
    assert_eq!(j["error"]["code"], "ssh_failed", "{j}");
    assert!(local.join("mid-down.bin.subak-part").exists());
    assert!(!down.exists());
    a.start();
    let d = call(with(&[
        "sftp",
        "get",
        &ta,
        &up_to,
        &down.display().to_string(),
        "--resume",
    ]));
    assert!(d["result"]["resumed_bytes"].as_u64().unwrap() > 0, "{d}");
    assert!(same_file(&mid, &down));

    // Deleting needs consent; mkdir refuses an existing name.
    let v = with(&["sftp", "rm", &ta, &up_to]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    lab.fails(&r, "confirmation_required");
    call(with(&["sftp", "rm", &ta, &up_to, "--yes"]));
    assert!(!dir.join("remote").join("mid.bin").exists());
    call(with(&["sftp", "mkdir", &ta, &format!("{remote}/made")]));
    assert!(dir.join("remote").join("made").is_dir());
    let v = with(&["sftp", "mkdir", &ta, &format!("{remote}/made")]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    lab.fails(&r, "file_exists");

    // One jump host (A to B) and two (A, then B, to C).
    let ls = call(with(&["sftp", "ls", &tb, &remote, "--jump", &ta]));
    assert!(ls["entries"].as_array().unwrap().len() >= 3, "{ls}");
    let tc = at(c3.port);
    call(with(&[
        "sftp", "ls", &tc, &remote, "--jump", &ta, "--jump", &tb,
    ]));

    // Forwards, held by SSH panes that run no remote command.
    let lf = format!("L:{lp}:127.0.0.1:{}", b.port);
    let d = call(with(&[
        "ssh",
        "connect",
        &ta,
        "fwd-local",
        "--forward",
        &lf,
        "--no-shell",
    ]));
    assert_eq!(d["ssh"]["forwards"][0]["listen"], "127.0.0.1", "{d}");
    let rf = format!("R:{rp}:127.0.0.1:{}", b.port);
    call(with(&[
        "ssh",
        "connect",
        &ta,
        "fwd-remote",
        "--forward",
        &rf,
        "--no-shell",
    ]));
    let df = format!("D:{dp}");
    call(with(&[
        "ssh",
        "connect",
        &ta,
        "fwd-socks",
        "--forward",
        &df,
        "--no-shell",
    ]));
    let wait_port = |p: u16| {
        let end = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", p)).is_err() {
            assert!(Instant::now() < end, "nothing listens on {p}");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    for p in [lp, rp, dp] {
        wait_port(p);
    }
    // B through each forward (the same host key, known under each port).
    call(with(&["sftp", "ls", &at(lp), &remote]));
    call(with(&["sftp", "ls", &at(rp), &remote]));
    call(with(&[
        "sftp",
        "ls",
        &tb,
        &remote,
        "--proxy",
        &format!("socks5://127.0.0.1:{dp}"),
    ]));
    let hp = http_proxy();
    call(with(&[
        "sftp",
        "ls",
        &tb,
        &remote,
        "--proxy",
        &format!("http://127.0.0.1:{hp}"),
    ]));

    // A taken port: refused before anything starts.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let tp = taken.local_addr().unwrap().port();
    let v = with(&[
        "ssh",
        "connect",
        &ta,
        "taken",
        "--forward",
        &format!("L:{tp}:127.0.0.1:1"),
        "--no-shell",
    ]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    lab.fails(&r, "port_in_use");
    drop(taken);

    // Unsafe forwarding is never the default and never silent.
    let v = with(&[
        "ssh",
        "connect",
        &ta,
        "pub",
        "--forward",
        "L:0.0.0.0:18089:127.0.0.1:1",
        "--dry-run",
    ]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    lab.fails(&r, "usage");
    let d = call(with(&[
        "ssh",
        "connect",
        &ta,
        "agent",
        "--agent-forward",
        "--dry-run",
    ]));
    assert_eq!(d["ssh"]["agent_forward"], true);
    assert!(
        d["ssh"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "ForwardAgent=yes")
    );
    assert!(
        d["ssh"]["warnings"][0]
            .as_str()
            .unwrap()
            .contains("agent forwarding"),
        "{d}"
    );
    let d = call(with(&["ssh", "connect", &ta, "plain", "--dry-run"]));
    assert!(
        d["ssh"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "ForwardAgent=no")
    );
    assert!(d["ssh"]["warnings"].as_array().unwrap().is_empty());
    let (code, doc) = {
        let v = with(&["ssh", "connect", &ta, "x", "--x11", "--dry-run"]);
        let r: Vec<&str> = v.iter().map(String::as_str).collect();
        // Without an X server (no DISPLAY), X11 forwarding is refused.
        let mut cmd = Command::new(subak());
        cmd.arg("--json")
            .args(["--endpoint", &lab.ep])
            .args(&r)
            .env("APPDATA", &lab.appdata)
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .creation_flags(NO_WINDOW);
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        )
    };
    assert_ne!(code, 0);
    assert_eq!(doc["error"]["code"], "invalid_state", "{doc}");
    for bad in ["ProxyJump=x", "LocalForward=1 h:2", "ForwardX11=yes"] {
        lab.fails(&["sftp", "ls", &ta, "--option", bad], "usage");
    }

    // A login script types into the pane after the expected text; every step is reported.
    let script = dir.join("login.toml");
    std::fs::write(
        &script,
        "[[step]]\nexpect = \"Microsoft\"\nsend = \"echo script-%OS%\"\n\n[[step]]\nexpect = \"script-Windows_NT\"\nsend = \"echo second-step\"\n",
    )
    .unwrap();
    let d = call(with(&[
        "ssh",
        "connect",
        &ta,
        "scripted",
        "--login-script",
        &script.display().to_string(),
    ]));
    assert_eq!(d["login_script"]["completed"], true, "{d}");
    assert_eq!(d["login_script"]["steps"][1]["sent"], "echo second-step");
    let end = Instant::now() + Duration::from_secs(20);
    // The command line and its output: two occurrences (the output may be the last line).
    while lab.screen("scripted").matches("second-step").count() < 2 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
    }
    let s = lab.screen("scripted");
    assert!(s.matches("second-step").count() >= 2, "{s}");
    let bad = dir.join("bad.toml");
    std::fs::write(&bad, "[[step]]\nexpect = \"$\"\nsend = \"export TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789\"\n").unwrap();
    let v = with(&[
        "ssh",
        "connect",
        &ta,
        "bad",
        "--login-script",
        &bad.display().to_string(),
    ]);
    let r: Vec<&str> = v.iter().map(String::as_str).collect();
    lab.fails(&r, "config_invalid");

    // Closing the forward panes ends their listeners.
    lab.ok(&["kill", "fwd-local", "--yes"]);
    let end = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", lp)).is_ok() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", lp)).is_err(),
        "the forward ended with its pane"
    );
}

//! E2E — Step 35: SFTP transfers, jump hosts and port forwarding (ADR 0068). `SUBAK-FR-061`,
//! `KIM-SEC-013`, `KIM-FR-013`.
//!
//! A user (or agent) with the release `subak` and two real SSH servers (Microsoft's portable
//! sshd from `scripts/fetch-test-sshd.ps1`, run as this user on 127.0.0.1 on a private desktop):
//! uploads a folder, lists it, downloads it back byte for byte, runs a background download and
//! follows it with `transfer status`, reaches the second server through the first as a jump
//! host and through a local forward held by an SSH pane, and is refused a taken port.
#![cfg(windows)]
#![allow(unsafe_code)]

use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[path = "../../crates/subak/tests/common/hidden.rs"]
mod hidden;

const NO_WINDOW: u32 = 0x0800_0000;

struct Env {
    exe: PathBuf,
    ep: String,
    appdata: PathBuf,
    dir: PathBuf,
    servers: Vec<u32>,
}

impl Env {
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let local = matches!(args.first(), Some(&("sftp" | "transfer")));
        let mut c = Command::new(&self.exe);
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
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code().unwrap_or(-1), doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.run(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.run(&["host", "stop", "--kill-terminals", "--yes"]);
        for p in self.servers.drain(..) {
            hidden::kill_tree(p);
        }
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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

fn wait_port(p: u16) {
    let end = Instant::now() + Duration::from_secs(20);
    while std::net::TcpStream::connect(("127.0.0.1", p)).is_err() {
        assert!(Instant::now() < end, "nothing listens on {p}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn keygen(bin: &Path, file: &Path) {
    assert!(
        Command::new(bin.join("ssh-keygen.exe"))
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .creation_flags(NO_WINDOW)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
#[ignore = "e2e: run via `cargo xtask e2e` only (AGENTS.md §4)"]
fn a_user_moves_files_over_sftp_through_jump_hosts_and_forwards() {
    let bin = std::env::var_os("SUBAK_TEST_SSHD_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-sshd/OpenSSH-Win64"),
        PathBuf::from,
    );
    assert!(
        bin.join("sshd.exe").is_file(),
        "no test sshd: run scripts/fetch-test-sshd.ps1 first ({})",
        bin.display()
    );
    let id = std::process::id();
    let dir = std::env::temp_dir().join(format!("subak-e2e35-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["appdata", "remote", "local", "tree/sub"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    let mut env = Env {
        exe: kim_e2e::bin("subak"),
        ep: format!("subak-e2e35-{id}"),
        appdata: dir.join("appdata"),
        dir: dir.clone(),
        servers: Vec::new(),
    };
    let (host, key) = (dir.join("host_ed25519"), dir.join("user_key"));
    keygen(&bin, &host);
    keygen(&bin, &key);
    std::fs::copy(key.with_extension("pub"), dir.join("authorized_keys")).unwrap();
    let host_pub = std::fs::read_to_string(host.with_extension("pub")).unwrap();
    let host_pub = host_pub
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let mut ports = Vec::new();
    for name in ["a", "b"] {
        let port = free_port();
        let cfg = dir.join(format!("sshd_{name}"));
        std::fs::write(
            &cfg,
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nStrictModes no\nPidFile {}\nAllowTcpForwarding yes\nSubsystem sftp {}\n",
                fwd(&host),
                fwd(&dir.join("authorized_keys")),
                fwd(&dir.join(format!("{name}.pid"))),
                fwd(&bin.join("sftp-server.exe"))
            ),
        )
        .unwrap();
        env.servers.push(hidden::spawn(
            &bin.join("sshd.exe"),
            &["-D".into(), "-f".into(), cfg.display().to_string()],
        ));
        wait_port(port);
        ports.push(port);
    }
    let lp = free_port();
    let kh = dir.join("known_hosts");
    std::fs::write(
        &kh,
        [ports[0], ports[1], lp]
            .iter()
            .map(|p| format!("[127.0.0.1]:{p} {host_pub}\n"))
            .collect::<String>(),
    )
    .unwrap();
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
    let (ta, tb) = (at(ports[0]), at(ports[1]));
    let remote = fwd(&dir.join("remote"));
    let run = |v: &[&str]| -> Value {
        let mut a = v.to_vec();
        a.extend(["--ssh-config", cfg_s.as_str()]);
        env.ok(&a)
    };

    // A folder up, listed, and back down byte for byte.
    std::fs::write(dir.join("tree").join("a.txt"), b"alpha").unwrap();
    let blob: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(dir.join("tree").join("sub").join("blob.bin"), &blob).unwrap();
    let tree = dir.join("tree").display().to_string();
    let d = run(&["sftp", "put", &ta, &tree, &remote, "--recursive"]);
    assert_eq!(d["result"]["files"], 2, "{d}");
    let ls = run(&["sftp", "ls", &ta, &format!("{remote}/tree")]);
    assert_eq!(ls["entries"][0]["name"], "a.txt", "{ls}");
    let local = dir.join("local").display().to_string();
    run(&[
        "sftp",
        "get",
        &ta,
        &format!("{remote}/tree"),
        &local,
        "--recursive",
    ]);
    assert_eq!(
        std::fs::read(dir.join("local/tree/sub/blob.bin")).unwrap(),
        blob
    );

    // A background download, followed to the end.
    let into = dir.join("local").join("again.bin").display().to_string();
    let d = run(&[
        "sftp",
        "get",
        &ta,
        &format!("{remote}/tree/sub/blob.bin"),
        &into,
        "--background",
    ]);
    let job = d["id"].as_str().unwrap().to_string();
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let j = env.ok(&["transfer", "status", &job]);
        if j["state"] == "done" {
            break;
        }
        assert!(
            Instant::now() < end && j["state"] != "failed",
            "transfer {job}: {j}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(std::fs::read(&into).unwrap(), blob);

    // The second server through the first: as a jump host, then through a local forward.
    run(&["sftp", "ls", &tb, &remote, "--jump", &ta]);
    let lf = format!("L:{lp}:127.0.0.1:{}", ports[1]);
    run(&[
        "ssh",
        "connect",
        &ta,
        "tunnel",
        "--forward",
        &lf,
        "--no-shell",
    ]);
    wait_port(lp);
    run(&["sftp", "ls", &at(lp), &remote]);

    // A taken port is refused before anything starts.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let tp = taken.local_addr().unwrap().port();
    let (code, doc) = env.run(&[
        "ssh",
        "connect",
        &ta,
        "taken",
        "--forward",
        &format!("L:{tp}:127.0.0.1:1"),
        "--no-shell",
        "--ssh-config",
        &cfg_s,
    ]);
    assert_eq!(code, 7, "{doc}");
    assert_eq!(doc["error"]["code"], "port_in_use");
}

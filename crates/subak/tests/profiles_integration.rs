//! Step 33 — shell profiles and the optional shell integration (ADR 0066). `SUBAK-FR-031`,
//! `SUBAK-FR-046`, `KIM-SEC-013`.
//!
//! Real Hosts and real shells: detected profiles start without configuration, a configured
//! profile's `title` names the Terminal, a missing shell is a clear error, and the shells report
//! their folder after `cd` — PowerShell through the block `subak shell-integration install`
//! writes (into a scratch profile file; the user's `$PROFILE` is never touched), cmd through a
//! profile's `PROMPT` — which `state.json` then keeps. Every run has its own `APPDATA`.
#![cfg(windows)]
#![allow(unsafe_code)]

#[path = "../../../xtask/src/jsonschema.rs"]
#[allow(dead_code)]
mod jsonschema;

mod common;

use common::*;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Lab {
    ep: String,
    appdata: PathBuf,
    dir: PathBuf,
    host: Option<Proc>,
}

impl Lab {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("subak-prof33-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let appdata = dir.join("appdata");
        std::fs::create_dir_all(appdata.join("mouv").join("subak")).unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        Self {
            ep: endpoint(tag),
            appdata,
            dir,
            host: None,
        }
    }

    fn config(&self, toml: &str) {
        std::fs::write(
            self.appdata.join("mouv").join("subak").join("config.toml"),
            toml,
        )
        .unwrap();
    }

    fn cli_raw(&self, args: &[&str]) -> (i32, Value) {
        // Commands that only read or edit files take no endpoint.
        let local = matches!(
            args.first(),
            Some(&("profile" | "shell-integration" | "config"))
        );
        let mut a = vec!["--json"];
        if !local {
            a.extend(["--endpoint", self.ep.as_str()]);
        }
        a.extend_from_slice(args);
        let (code, doc) = run_env(&a, &[("APPDATA", self.appdata.to_str().unwrap())]);
        validate("subak-cli-v1", &doc);
        (code, doc)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, doc) = self.cli_raw(args);
        assert_eq!(code, 0, "{args:?}: {doc}");
        doc["data"].clone()
    }

    fn start(&mut self) {
        let sd = self.dir.join("state").display().to_string();
        let d = self.ok(&[
            "host",
            "start",
            "--no-shell",
            "--idle-exit",
            "60",
            "--state-dir",
            &sd,
        ]);
        self.host = Some(Proc::open(pid_of(&d["host"]["pid"])));
    }

    fn current_dir(&self, t: &str) -> Value {
        self.ok(&["info", t])["terminal"]["windows"][0]["panes"][0]["current_dir"].clone()
    }

    fn wait_dir(&self, t: &str, want: &str) {
        let end = Instant::now() + LONG;
        while self.current_dir(t).as_str().map(str::to_lowercase) != Some(want.to_lowercase()) {
            assert!(
                Instant::now() < end,
                "{t} never reported {want}: {}",
                self.current_dir(t)
            );
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        if let Some(h) = &self.host {
            let _ = run(&[
                "host",
                "stop",
                "--endpoint",
                &self.ep,
                "--kill-terminals",
                "--yes",
            ]);
            if !h.wait(Duration::from_secs(20)) {
                h.kill();
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn detected_and_configured_profiles_start_real_shells_and_missing_ones_fail_clearly() {
    let mut lab = Lab::new("w33-prof");
    let work = lab.dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    lab.config(&format!(
        "config_version = 1\n\n[[profiles]]\nname = \"Build\"\nprogram = \"cmd.exe\"\nargs = [\"/d\", \"/k\", \"echo build-profile\"]\ncwd = '{}'\ntitle = \"build\"\n\n[[profiles]]\nname = \"Gone\"\nprogram = 'C:\\nowhere\\noshell.exe'\n",
        work.display()
    ));
    let list = lab.ok(&["profile", "list"]);
    let profiles = list["profiles"].as_array().unwrap();
    let names: Vec<&str> = profiles
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"Build")
            && names.contains(&"Command Prompt")
            && names.contains(&"Windows PowerShell"),
        "{names:?}"
    );
    assert_eq!(
        profiles
            .iter()
            .find(|p| p["name"] == "Command Prompt")
            .unwrap()["source"],
        "detected"
    );
    assert_eq!(
        profiles.iter().find(|p| p["name"] == "Build").unwrap()["source"],
        "config"
    );

    lab.start();
    // A detected profile starts its shell; nothing had to be configured.
    let r = lab.ok(&["new", "ps", "--profile", "Windows PowerShell"]);
    assert_eq!(r["profile"], "Windows PowerShell");
    assert!(
        r["terminal"]["windows"][0]["panes"][0]["executable"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .ends_with("powershell.exe")
    );
    // The title names the Terminal; when it is taken, a free name is used.
    let r = lab.ok(&["new", "--profile", "build"]);
    assert_eq!(r["name"], "build");
    let r2 = lab.ok(&["new", "--profile", "Build"]);
    assert_ne!(r2["name"], "build");
    // A configured shell that does not exist: a clear error, nothing left behind.
    let (code, doc) = lab.cli_raw(&["new", "--profile", "Gone"]);
    assert_ne!(code, 0);
    assert_eq!(doc["error"]["code"], "spawn_failed", "{doc}");
    assert_eq!(lab.ok(&["ls"])["terminals"].as_array().unwrap().len(), 3);
    let (code, doc) = lab.cli_raw(&["new", "--profile", "No such profile"]);
    assert_eq!(
        (code, doc["error"]["code"].clone()),
        (3, json!("profile_not_found"))
    );
    // Detection can be turned off.
    lab.config("config_version = 1\n\n[terminal]\ndetect_shells = false\n");
    let list = lab.ok(&["profile", "list"]);
    assert_eq!(list["profiles"], json!([]));
}

#[test]
fn shells_report_their_folder_with_the_optional_integration_and_state_keeps_it() {
    let mut lab = Lab::new("w33-int");
    let ps1 = lab.dir.join("profile.ps1");
    std::fs::write(&ps1, "# my own profile\r\n$mine = 1\r\n").unwrap();
    let f = ps1.display().to_string();
    let r = lab.ok(&[
        "shell-integration",
        "install",
        "powershell",
        "--profile-file",
        &f,
    ]);
    assert_eq!(
        (r["changed"].clone(), r["installed"].clone()),
        (json!(true), json!(true))
    );
    assert!(
        std::fs::read_to_string(&ps1)
            .unwrap()
            .starts_with("# my own profile\r\n$mine = 1\r\n")
    );
    assert_eq!(
        lab.ok(&[
            "shell-integration",
            "status",
            "powershell",
            "--profile-file",
            &f
        ])["installed"],
        true
    );

    lab.start();
    let target = std::env::temp_dir();
    let tdir = target
        .display()
        .to_string()
        .trim_end_matches('\\')
        .to_string();
    // PowerShell runs the scratch profile (never the user's own). `-ExecutionPolicy Bypass`
    // applies to this one process only: this PC's policy refuses unsigned scripts, and a real
    // $PROFILE runs only where the user's policy allows it (Subak never changes the policy).
    let line = format!(
        "powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -NoExit -Command \". '{f}'\""
    );
    lab.ok(&["new", "ps", "--command-line", &line]);
    lab.ok(&[
        "command",
        "--target",
        "ps",
        &format!("send-keys -l 'cd \"{tdir}\"'"),
    ]);
    lab.ok(&["command", "--target", "ps", "send-keys Enter"]);
    lab.wait_dir("ps", &tdir);
    // cmd: a profile's PROMPT opts in; nothing global changes.
    lab.ok(&[
        "new",
        "c",
        "--env",
        "PROMPT=$e]9;9;$P$e\\$P$G",
        "--",
        "cmd.exe",
        "/d",
    ]);
    lab.ok(&[
        "command",
        "--target",
        "c",
        "send-keys -l 'cd /d C:\\Windows'",
    ]);
    lab.ok(&["command", "--target", "c", "send-keys Enter"]);
    lab.wait_dir("c", r"C:\Windows");
    // The state keeps the folder the shell moved to.
    let state = lab.dir.join("state").join("state.json");
    let end = Instant::now() + LONG;
    loop {
        let t = std::fs::read_to_string(&state).unwrap_or_default();
        if t.contains("C:\\\\Windows") {
            break;
        }
        assert!(Instant::now() < end, "state.json never kept the new folder");
        std::thread::sleep(Duration::from_millis(150));
    }
    // Uninstall gives the user's profile back, byte for byte.
    let r = lab.ok(&[
        "shell-integration",
        "uninstall",
        "powershell",
        "--profile-file",
        &f,
    ]);
    assert_eq!(r["changed"], true);
    assert_eq!(
        std::fs::read_to_string(&ps1).unwrap(),
        "# my own profile\r\n$mine = 1\r\n"
    );
    let (code, _) = lab.cli_raw(&["shell-integration", "install", "cmd"]);
    assert_eq!(code, 2, "cmd has no file Subak would change");
}

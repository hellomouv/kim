//! Step 33 (ADR 0066): the local shells found on this PC, offered as profiles next to those of
//! `config.toml`. Detection only reads well-known paths and the registry; it never runs a shell
//! (or anything else) to probe, and it changes nothing.

use crate::config::Profile;
use std::path::{Path, PathBuf};

fn profile(name: &str, program: &Path, args: &[&str]) -> Profile {
    Profile {
        name: name.to_string(),
        program: program.display().to_string(),
        args: args.iter().map(|a| (*a).to_string()).collect(),
        cwd: None,
        env: Vec::new(),
        unset_env: Vec::new(),
        title: None,
        detected: true,
        ssh: None,
        identity: None,
        jump: Vec::new(),
        forwards: Vec::new(),
        agent_forward: false,
        login_script: None,
        group: Some("Detected".into()),
        line: None,
    }
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// `name` on the `PATH`, first match (only existing files; nothing is started).
fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The shells found here, in a stable order: PowerShell 7, Windows PowerShell, Command Prompt,
/// Git Bash, then one profile per WSL distribution.
pub fn detect() -> Vec<Profile> {
    // Golden tests need the same output on every machine (debug builds only).
    #[cfg(debug_assertions)]
    if std::env::var_os("SUBAK_TEST_NO_DETECT").is_some() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let pf = env_dir("ProgramFiles");
    let sys = env_dir("SystemRoot").map(|r| r.join("System32"));
    // PowerShell 7 (and newer): Program Files, else the PATH (a per-user or store install).
    let pwsh = pf
        .as_ref()
        .and_then(|p| {
            ["7", "7-preview"]
                .iter()
                .map(|v| p.join("PowerShell").join(v).join("pwsh.exe"))
                .find(|x| x.is_file())
        })
        .or_else(|| on_path("pwsh.exe"));
    if let Some(p) = pwsh {
        out.push(profile("PowerShell 7", &p, &["-NoLogo"]));
    }
    if let Some(p) = sys
        .as_ref()
        .map(|s| {
            s.join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe")
        })
        .filter(|p| p.is_file())
    {
        out.push(profile("Windows PowerShell", &p, &["-NoLogo"]));
    }
    if let Some(p) = sys
        .as_ref()
        .map(|s| s.join("cmd.exe"))
        .filter(|p| p.is_file())
    {
        out.push(profile("Command Prompt", &p, &[]));
    }
    let git = [
        pf,
        env_dir("ProgramFiles(x86)"),
        env_dir("LOCALAPPDATA").map(|l| l.join("Programs")),
    ]
    .into_iter()
    .flatten()
    .map(|d| d.join("Git").join("bin").join("bash.exe"))
    .find(|p| p.is_file());
    if let Some(p) = git {
        out.push(profile("Git Bash", &p, &["--login", "-i"]));
    }
    // #98 T4: Clink, Cygwin, MSYS2, Cmder and the Visual Studio developer prompts.
    out.extend(extra(&Roots::from_env()));
    #[cfg(windows)]
    if let Some(wsl) = sys
        .as_ref()
        .map(|s| s.join("wsl.exe"))
        .filter(|p| p.is_file())
    {
        for d in crate::win::wsl_distributions() {
            out.push(profile(&format!("WSL: {d}"), &wsl, &["-d", &d]));
        }
    }
    out
}

/// #98 T4: where the extra shells are looked for (from the environment; a test passes its own
/// folders).
#[derive(Debug, Default)]
pub struct Roots {
    /// `%SystemDrive%\` (Cygwin, MSYS2 and Cmder's usual places).
    pub drive: Option<PathBuf>,
    pub program_files: Option<PathBuf>,
    pub program_files_x86: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    pub user_profile: Option<PathBuf>,
    /// `%CMDER_ROOT%`, when the user set it.
    pub cmder_root: Option<PathBuf>,
    /// `System32` (for `cmd.exe`).
    pub system32: Option<PathBuf>,
}

impl Roots {
    fn from_env() -> Roots {
        Roots {
            drive: std::env::var_os("SystemDrive")
                .map(|d| PathBuf::from(format!("{}\\", d.to_string_lossy())))
                .filter(|p| p.is_dir()),
            program_files: env_dir("ProgramFiles"),
            program_files_x86: env_dir("ProgramFiles(x86)"),
            local_app_data: env_dir("LOCALAPPDATA"),
            program_data: env_dir("ProgramData"),
            user_profile: env_dir("USERPROFILE"),
            cmder_root: env_dir("CMDER_ROOT"),
            system32: env_dir("SystemRoot").map(|r| r.join("System32")),
        }
    }
}

/// `cmd.exe /k <script> [extra]` that Command Prompt reads right: a script path without its
/// special characters is passed as it is (quoted when it has spaces); one with them (`(x86)`)
/// starts in the script's folder and runs it by name. The arguments and the working folder.
fn cmd_k(script: &Path, extra: &str) -> (Vec<String>, Option<String>) {
    let plain = !script
        .to_string_lossy()
        .contains(['&', '<', '>', '(', ')', '@', '^', '|']);
    if plain {
        let mut args = vec!["/k".to_string(), script.display().to_string()];
        if !extra.is_empty() {
            args.push(extra.to_string());
        }
        (args, None)
    } else {
        let name = script
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let line = if extra.is_empty() {
            name
        } else {
            format!("{name} {extra}")
        };
        (
            vec!["/k".to_string(), line],
            script.parent().map(|d| d.display().to_string()),
        )
    }
}

/// A Command Prompt profile that runs `script` first.
fn cmd_profile(name: &str, cmd: &Path, script: &Path, extra: &str) -> Profile {
    let (args, cwd) = cmd_k(script, extra);
    let mut p = profile(name, cmd, &[]);
    p.args = args;
    p.cwd = cwd;
    p
}

/// #98 T4 (`SHELL-04`, `SHELL-08`–`SHELL-11`): the shells that need an install of their own,
/// found by their files only (nothing is run to look): Command Prompt with the user's Clink,
/// Cygwin, MSYS2 (one profile per environment), Cmder and the Visual Studio developer prompts.
pub fn extra(r: &Roots) -> Vec<Profile> {
    let mut out = Vec::new();
    let Some(cmd) = r
        .system32
        .as_ref()
        .map(|s| s.join("cmd.exe"))
        .filter(|p| p.is_file())
    else {
        return out;
    };
    // Clink: its clink.bat, in Program Files or the user's local programs.
    if let Some(bat) = [&r.program_files_x86, &r.program_files, &r.local_app_data]
        .into_iter()
        .flatten()
        .flat_map(|d| {
            [
                d.join("clink").join("clink.bat"),
                d.join("Programs").join("clink").join("clink.bat"),
            ]
        })
        .find(|p| p.is_file())
    {
        out.push(cmd_profile("Command Prompt (Clink)", &cmd, &bat, "inject"));
    }
    // Cygwin: its bash, starting in the folder Subak gives it (CHERE_INVOKING).
    if let Some(bash) = r
        .drive
        .iter()
        .flat_map(|d| [d.join("cygwin64"), d.join("cygwin")])
        .map(|c| c.join("bin").join("bash.exe"))
        .find(|p| p.is_file())
    {
        let mut p = profile("Cygwin", &bash, &["--login", "-i"]);
        p.env = vec![("CHERE_INVOKING".into(), "1".into())];
        out.push(p);
    }
    // MSYS2: one profile per environment, its bash with MSYSTEM.
    if let Some(bash) = r
        .drive
        .iter()
        .map(|d| d.join("msys64").join("usr").join("bin").join("bash.exe"))
        .find(|p| p.is_file())
    {
        for env in ["UCRT64", "MINGW64", "CLANG64", "MSYS"] {
            let mut p = profile(&format!("MSYS2 {env}"), &bash, &["--login", "-i"]);
            p.env = vec![
                ("CHERE_INVOKING".into(), "1".into()),
                ("MSYSTEM".into(), env.into()),
            ];
            out.push(p);
        }
    }
    // Cmder: %CMDER_ROOT%, else its usual folders; its vendor\init.bat.
    if let Some(init) = r
        .cmder_root
        .iter()
        .cloned()
        .chain(r.drive.iter().map(|d| d.join("tools").join("cmder")))
        .chain(r.user_profile.iter().map(|u| u.join("cmder")))
        .map(|c| c.join("vendor").join("init.bat"))
        .find(|p| p.is_file())
    {
        out.push(cmd_profile("Cmder", &cmd, &init, ""));
    }
    // Visual Studio: each installed instance (its state.json), with VsDevCmd.bat.
    if let Some(dir) = r.program_data.as_ref().map(|d| {
        d.join("Microsoft")
            .join("VisualStudio")
            .join("Packages")
            .join("_Instances")
    }) {
        let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let text = std::fs::read_to_string(e.path().join("state.json")).ok()?;
                let v: serde_json::Value =
                    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
                let path = PathBuf::from(v["installationPath"].as_str()?);
                let title = v["localizedResources"]
                    .as_array()
                    .and_then(|rs| {
                        rs.iter()
                            .find(|r| r["language"] == "en-us")
                            .or_else(|| rs.first())
                    })
                    .and_then(|r| r["title"].as_str())
                    .map_or_else(|| "Visual Studio".to_string(), String::from);
                let bat = path.join("Common7").join("Tools").join("VsDevCmd.bat");
                bat.is_file().then_some((title, bat))
            })
            .collect();
        found.sort();
        found.dedup_by(|a, b| a.0 == b.0);
        for (title, bat) in found {
            out.push(cmd_profile(
                &format!("Developer Prompt for {title}"),
                &cmd,
                &bat,
                "",
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_finds_this_pcs_shells_without_running_anything() {
        let found = detect();
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        // Every Windows has these two.
        assert!(names.contains(&"Command Prompt"), "{names:?}");
        assert!(names.contains(&"Windows PowerShell"), "{names:?}");
        for p in &found {
            assert!(p.detected && Path::new(&p.program).is_file(), "{p:?}");
            // #98 T4: only what a shell needs to start in Subak's folder (MSYS2's environment,
            // CHERE_INVOKING), and a script's folder when Command Prompt cannot quote it.
            assert!(
                p.env
                    .iter()
                    .all(|(n, _)| n == "CHERE_INVOKING" || n == "MSYSTEM"),
                "nothing more is assumed: {p:?}"
            );
            assert!(p.cwd.is_none() || p.program.ends_with("cmd.exe"), "{p:?}");
        }
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "names are unique");
    }

    /// #98 T4 (`SHELL-04`, `SHELL-08`–`SHELL-11`): the extra shells are found by their files in
    /// a fake tree; Command Prompt gets scripts it can read (a `(x86)` path runs by name from
    /// its folder).
    #[test]
    fn the_extra_shells_are_found_by_their_files() {
        let root = std::env::temp_dir().join(format!("subak-shells-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let touch = |p: PathBuf| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"").unwrap();
            p
        };
        let cmd = touch(root.join("Windows/System32/cmd.exe"));
        let clink = touch(root.join("Program Files (x86)/clink/clink.bat"));
        touch(root.join("cygwin64/bin/bash.exe"));
        touch(root.join("msys64/usr/bin/bash.exe"));
        let init = touch(root.join("tools/cmder/vendor/init.bat"));
        let vs = root.join("VS/2022/Community");
        touch(vs.join("Common7/Tools/VsDevCmd.bat"));
        let state =
            root.join("ProgramData/Microsoft/VisualStudio/Packages/_Instances/abc/state.json");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        std::fs::write(
            &state,
            serde_json::json!({"installationPath": vs.display().to_string(),
                "localizedResources": [{"language": "en-us", "title": "Visual Studio Community 2022"}]})
                .to_string(),
        )
        .unwrap();
        let r = Roots {
            drive: Some(root.clone()),
            program_files: Some(root.join("Program Files")),
            program_files_x86: Some(root.join("Program Files (x86)")),
            local_app_data: None,
            program_data: Some(root.join("ProgramData")),
            user_profile: None,
            cmder_root: None,
            system32: Some(cmd.parent().unwrap().to_path_buf()),
        };
        let found = extra(&r);
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Command Prompt (Clink)",
                "Cygwin",
                "MSYS2 UCRT64",
                "MSYS2 MINGW64",
                "MSYS2 CLANG64",
                "MSYS2 MSYS",
                "Cmder",
                "Developer Prompt for Visual Studio Community 2022",
            ]
        );
        // Clink under `(x86)`: run by name from its folder.
        assert_eq!(found[0].args, ["/k", "clink.bat inject"]);
        assert_eq!(found[0].cwd.as_deref().map(Path::new), clink.parent());
        // Cmder's path is plain: passed as it is.
        assert_eq!(found[6].args[0], "/k");
        assert_eq!(Path::new(&found[6].args[1]), init.as_path());
        assert_eq!(found[6].cwd, None);
        assert!(found[2].env.contains(&("MSYSTEM".into(), "UCRT64".into())));
        assert!(found.iter().all(|p| p.detected));
        assert!(
            extra(&Roots::default()).is_empty(),
            "no Command Prompt: nothing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

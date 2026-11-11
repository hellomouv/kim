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
            assert!(
                p.env.is_empty() && p.cwd.is_none(),
                "nothing more is assumed"
            );
        }
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "names are unique");
    }
}

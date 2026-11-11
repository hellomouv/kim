//! Step 33 (ADR 0066): the optional shell integration — the shell reports its working directory
//! (OSC 9;9) at every prompt, so Subak knows a pane's folder after `cd` (shown as
//! `current_dir`, saved in `state.json`, used by restore).
//!
//! - **PowerShell** (Windows PowerShell 5.1 and PowerShell 7): `subak shell-integration install`
//!   adds one marked block to the user's `$PROFILE` (a backup first), active only inside a
//!   Subak pane (`$env:SUBAK_PANE_ID`); `uninstall` removes exactly that block. Nothing else in
//!   the profile changes, and nothing happens without the command.
//! - **cmd**: no file is touched (cmd has no per-user profile; its AutoRun registry hook would be
//!   global). A profile opts in with an environment value, shown by `status cmd`.

use crate::cli::{Args, Cmd};
use crate::output::{CliError, Code};
use serde_json::{Value, json};
use std::path::PathBuf;

pub const BEGIN: &str = "# >>> subak shell integration >>> (managed by `subak shell-integration`; remove it with `subak shell-integration uninstall`)";
pub const END: &str = "# <<< subak shell integration <<<";

/// The PowerShell block: it wraps the current prompt function, only inside Subak panes.
pub fn powershell_block(nl: &str) -> String {
    [
        BEGIN,
        "if ($env:SUBAK_PANE_ID -and -not $global:__subak_prompt) {",
        "    $global:__subak_prompt = $function:prompt",
        "    function global:prompt {",
        "        $loc = $executionContext.SessionState.Path.CurrentLocation",
        "        if ($loc.Provider.Name -eq 'FileSystem') {",
        "            [Console]::Write(\"$([char]27)]9;9;`\"$($loc.ProviderPath)`\"$([char]27)\\\")",
        "        }",
        "        & $global:__subak_prompt",
        "    }",
        "}",
        END,
    ]
    .join(nl)
}

/// The per-profile opt-in for cmd: `PROMPT` with the report and cmd's default look.
pub const CMD_PROMPT: &str = "$e]9;9;$P$e\\$P$G";

/// PowerShell runs a profile only when its execution policy allows scripts; Subak never changes
/// the policy (a system security setting).
pub const POLICY_NOTE: &str = "PowerShell runs this profile only when its execution policy allows scripts (`Get-ExecutionPolicy -List`); Subak never changes the policy";

/// Whether `text` holds Subak's block.
pub fn installed(text: &str) -> bool {
    text.contains(BEGIN) && text.contains(END)
}

/// `text` with Subak's block added at its end (unchanged when it is there).
pub fn add(text: &str) -> (String, bool) {
    if installed(text) {
        return (text.to_string(), false);
    }
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(nl);
    }
    out.push_str(&powershell_block(nl));
    out.push_str(nl);
    (out, true)
}

/// `text` without Subak's block (and the line end it added); everything else stays.
pub fn remove(text: &str) -> (String, bool) {
    let (Some(b), Some(e)) = (text.find(BEGIN), text.find(END)) else {
        return (text.to_string(), false);
    };
    if e < b {
        return (text.to_string(), false);
    }
    let mut end = e + END.len();
    if text[end..].starts_with("\r\n") {
        end += 2;
    } else if text[end..].starts_with('\n') {
        end += 1;
    }
    (format!("{}{}", &text[..b], &text[end..]), true)
}

/// The user's PowerShell profile file for `shell` (`powershell` or `pwsh`): the current user's,
/// current host's `$PROFILE` under the Documents folder.
fn profile_file(shell: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let docs = crate::win::documents_dir();
    #[cfg(not(windows))]
    let docs: Option<PathBuf> = None;
    let docs = docs
        .or_else(|| std::env::var_os("USERPROFILE").map(|u| PathBuf::from(u).join("Documents")))?;
    let folder = if shell == "pwsh" {
        "PowerShell"
    } else {
        "WindowsPowerShell"
    };
    Some(docs.join(folder).join("Microsoft.PowerShell_profile.ps1"))
}

/// `subak shell-integration status|install|uninstall [powershell|pwsh|cmd] [--profile-file <f>]`.
pub fn run(cmd: Cmd, a: &Args) -> Result<(Value, String), CliError> {
    let shell = a
        .positional
        .first()
        .map_or("powershell", String::as_str)
        .to_ascii_lowercase();
    if shell == "cmd" {
        if cmd != Cmd::ShellIntegrationStatus {
            return Err(CliError::usage(
                "cmd has no profile file Subak would change; opt in per profile instead",
            )
            .hint(format!(
                "add `env = {{ PROMPT = '{CMD_PROMPT}' }}` to a [[profiles]] table of config.toml"
            )));
        }
        let data = json!({"shell": "cmd", "installed": false, "file": null,
            "opt_in": {"profile_env": {"PROMPT": CMD_PROMPT}}});
        return Ok((
            data,
            format!(
                "cmd: nothing is installed; a profile opts in with env PROMPT = '{CMD_PROMPT}'\n"
            ),
        ));
    }
    if shell != "powershell" && shell != "pwsh" {
        return Err(CliError::usage(format!(
            "`{shell}` is not powershell, pwsh or cmd"
        )));
    }
    let file = a
        .settings
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| profile_file(&shell))
        .ok_or_else(|| CliError::new(Code::Io, "cannot find the Documents folder"))?;
    let shown = file.display().to_string();
    let raw = match std::fs::read(&file) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(CliError::new(
                Code::Io,
                format!("cannot read `{shown}`: {e}"),
            ));
        }
    };
    let bom = raw
        .as_ref()
        .is_some_and(|b| b.starts_with(&[0xEF, 0xBB, 0xBF]));
    let text = match &raw {
        Some(b) => String::from_utf8(b[if bom { 3 } else { 0 }..].to_vec()).map_err(|_| {
            CliError::new(
                Code::ConfigInvalid,
                format!("`{shown}` is not UTF-8; nothing was changed"),
            )
            .hint("save it as UTF-8 in an editor, or edit it yourself")
        })?,
        None => String::new(),
    };
    if cmd == Cmd::ShellIntegrationStatus {
        let data = json!({"shell": shell, "file": shown, "exists": raw.is_some(), "installed": installed(&text),
            "note": POLICY_NOTE});
        let h = format!(
            "{shell}: {} ({shown})\nnote: {POLICY_NOTE}\n",
            if installed(&text) {
                "installed"
            } else {
                "not installed"
            }
        );
        return Ok((data, h));
    }
    let (new, changed) = if cmd == Cmd::ShellIntegrationInstall {
        add(&text)
    } else {
        remove(&text)
    };
    let mut data = json!({"shell": shell, "file": shown, "changed": changed, "dry_run": a.dry_run, "backup": null,
        "installed": installed(&new), "note": POLICY_NOTE});
    if !changed || a.dry_run {
        let h = format!(
            "{shown}: {}\n",
            if !changed {
                "nothing to change"
            } else if cmd == Cmd::ShellIntegrationInstall {
                "would add Subak's block"
            } else {
                "would remove Subak's block"
            }
        );
        return Ok((data, h));
    }
    if let Some(b) = &raw {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut n = 0;
        let backup = loop {
            let p = PathBuf::from(if n == 0 {
                format!("{shown}.subak-backup-{stamp}")
            } else {
                format!("{shown}.subak-backup-{stamp}-{n}")
            });
            if !p.exists() {
                break p;
            }
            n += 1;
        };
        std::fs::write(&backup, b).map_err(|e| {
            CliError::new(
                Code::Io,
                format!("cannot write the backup: {e}; nothing was changed"),
            )
        })?;
        data["backup"] = json!(backup.display().to_string());
    } else if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            CliError::new(Code::Io, format!("cannot create `{}`: {e}", dir.display()))
        })?;
    }
    let mut bytes = if bom {
        vec![0xEF, 0xBB, 0xBF]
    } else {
        Vec::new()
    };
    bytes.extend_from_slice(new.as_bytes());
    std::fs::write(&file, bytes)
        .map_err(|e| CliError::new(Code::Io, format!("cannot write `{shown}`: {e}")))?;
    let h = format!(
        "{shown}: {} Subak's block{}; new PowerShell sessions in Subak panes report their folder\nnote: {POLICY_NOTE}\n",
        if cmd == Cmd::ShellIntegrationInstall {
            "added"
        } else {
            "removed"
        },
        data["backup"]
            .as_str()
            .map_or(String::new(), |b| format!(" (backup: {b})"))
    );
    Ok((data, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_is_added_once_and_removed_exactly() {
        let mine = "Set-Alias ll Get-ChildItem\r\n# my prompt\r\nfunction prompt { 'PS> ' }\r\n";
        let (with, changed) = add(mine);
        assert!(changed && installed(&with));
        assert!(
            with.starts_with(mine),
            "the user's lines stay first, unchanged"
        );
        assert!(with.contains("\r\n$env:SUBAK_PANE_ID") || with.contains("if ($env:SUBAK_PANE_ID"));
        assert!(!with.replace("\r\n", "").contains('\n'), "CRLF kept");
        assert_eq!(add(&with), (with.clone(), false), "idempotent");
        let (back, removed) = remove(&with);
        assert!(removed);
        assert_eq!(back, mine, "exactly the user's profile again");
        assert_eq!(remove(mine), (mine.to_string(), false));
        let (fresh, _) = add("");
        assert_eq!(remove(&fresh).0, "");
        assert!(
            powershell_block("\n").contains("]9;9;"),
            "reports with OSC 9;9"
        );
    }
}

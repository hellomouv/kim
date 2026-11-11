# ADR 0066 — Detected shell profiles and an opt-in shell integration

- **Status:** Accepted (Step 33, 2026-11-11; the visible window pending the maintainer)
- **Requirements:** `SUBAK-FR-031`, `SUBAK-FR-046`, `KIM-SEC-013`, `KIM-SEC-001`
- **Builds on:** [ADR 0051](0051-subak-settings-profiles-and-appearance.md) (profiles in
  `config.toml`), [ADR 0065](0065-tabby-parity-priorities-and-ssh-transport-criteria.md)

## Decision

1. **Detected shells are profiles without configuration.** PowerShell 7 (Program Files 7 and
   7-preview, else the `PATH`), Windows PowerShell, Command Prompt, Git Bash (Program Files,
   Program Files (x86), the per-user Programs folder) and one `WSL: <distro>` per distribution
   registered for the user (`HKCU\…\Lxss\*\DistributionName`, read only). Detection reads paths
   and the registry only — it never runs a shell, `wsl.exe` or anything else — and is never
   written to `config.toml`. A configured profile with the same name wins;
   `terminal.detect_shells = false` turns detection off. `subak profile list|show`, `subak new
   --profile`, `default_profile` and the window's menu use configured and detected profiles alike
   (`source: config | detected`).
2. **`title`** (optional, a valid Terminal name) names a new Terminal of the profile when the user
   gives no name; when that name is taken, the Host picks a free one.
3. **A missing shell** (a configured program that does not exist) fails with `spawn_failed` and
   leaves nothing behind; an unknown profile is `profile_not_found` with the list of names.
4. **Shell integration is optional and explicit.** Its only job (for now) is the working
   directory: the shell reports it at each prompt with OSC 9;9 (OSC 7 is read too), Subak shows it
   as the pane's `current_dir` and `state.json` keeps it, so a restored pane starts where the
   shell last was.
   - PowerShell: `subak shell-integration install [powershell|pwsh]` adds one marked block to the
     current user's `$PROFILE` (Documents folder from the registry, so a redirected one works),
     after a byte-exact backup and keeping its encoding (BOM) and line ends; the block is active
     only when `$env:SUBAK_PANE_ID` is set (inside Subak panes) and wraps the existing prompt.
     `uninstall` removes exactly the block. PowerShell runs a profile only where its execution
     policy allows scripts; Subak says so and **never changes the policy**.
   - cmd: nothing is installed (cmd has no per-user profile; the AutoRun registry hook would be
     global). A profile opts in with `env = { PROMPT = '$e]9;9;$P$e\$P$G' }`.
   - No command marks (OSC 133), completion or suggestions: they are not reliable across shells
     yet (follow-ups in the Tabby matrix).
5. **Keyboard:** `Ctrl+B N` (`terminal.new-from`) opens the menu of profiles; `Ctrl+Shift+T`
   stays a key for programs.

## Consequences

- A PC without PowerShell 7 simply shows no such profile; Store installs outside the `PATH` are
  not found yet.
- Profiles travel with `subak config export`/`import` (versioned `config.toml`); detected ones are
  re-detected on the other PC, never exported.

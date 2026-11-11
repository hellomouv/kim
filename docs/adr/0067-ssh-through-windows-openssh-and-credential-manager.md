# ADR 0067 — SSH through Windows' OpenSSH client, secrets in Credential Manager

- **Status:** Accepted (Step 34, 2026-11-11)
- **Requirements:** `SUBAK-FR-061`, `KIM-SEC-013`, `KIM-SEC-001`
- **Builds on:** [ADR 0065](0065-tabby-parity-priorities-and-ssh-transport-criteria.md) §4 (the
  SSH transport criteria), [ADR 0066](0066-detected-shell-profiles-and-shell-integration.md)

## Decision

1. **Transport: Windows' own OpenSSH client** (`%SystemRoot%\System32\OpenSSH\ssh.exe`, a
   Windows optional feature serviced by Windows Update) runs as the pane's program. Against the
   ADR 0065 criteria:
   - *Security maintenance:* Microsoft ships and patches it; Subak adds no cryptography of its own
     and no C build dependency (`russh` needs `ring` or `aws-lc`, which do not build with this
     toolchain without a C compiler — tried in Step 34).
   - *Compatibility:* the user's `~/.ssh/config`, `known_hosts`, keys (OpenSSH and PEM), the Windows
     ssh-agent, keyboard-interactive and 2FA prompts all work as they do in any terminal.
   - *Lightweight:* no new crate; an SSH pane is a Terminal like any other (tabs, splits, detach,
     GUI close, the Host lifecycle).
   - *Cost:* Subak cannot draw its own host-key or password dialogs; they are ssh's prompts in
     the pane. `.ppk` keys and Pageant are not supported by this client (Step 36).
   - If the client is missing, `ssh connect` fails with `spawn_failed` and says how to add the
     Windows feature; **Subak never installs it**.
2. **Host keys are never trusted silently.** Every SSH pane runs with
   `StrictHostKeyChecking=ask`: a new host shows OpenSSH's fingerprint question, which the user
   answers; a changed key stops the connection with OpenSSH's warning. `--option` refuses
   `StrictHostKeyChecking`, `ProxyCommand`, `LocalCommand`, `PermitLocalCommand`,
   `KnownHostsCommand`, `ForwardAgent`, `Match` and `Include` (use `ssh` directly for those).
3. **Nothing is forwarded by default:** `ForwardAgent=no`; no secret is ever sent to a remote
   command, an MCP server or an agent.
4. **Secrets live only in Windows Credential Manager** (generic credentials named
   `mouv-subak:ssh:<user>@<host>:<port>`, DPAPI-protected for the user), saved by an explicit
   `subak ssh credential save` and only for a destination already in `known_hosts`. The secret is
   read without echo (or from one stdin line for scripts) and is never in a command line, an
   environment variable, `config.toml`, `state.json`, IPC, a log or JSON output.
5. **Handing the secret to ssh:** for a verified destination with a saved secret, the pane gets
   `SSH_ASKPASS=<subak.exe>`, `SSH_ASKPASS_REQUIRE=force`, `SUBAK_ASKPASS=1` and the credential
   *name*. When ssh asks, `subak.exe <prompt>` answers **only** a password or passphrase prompt,
   **once per ssh process** (its id and start time); a second prompt — a wrong or expired secret —
   is refused, so ssh fails visibly instead of looping. Yes/no and fingerprint questions are never
   answered.
6. **Failure UX:** SSH panes keep their last screen when ssh ends (remain-on-exit), so a refused
   key, a changed host key or an offline server stays readable; reconnecting is connecting again.
   A Reconnect action, auto-reconnect and disconnected tabs after a reboot are Step 36.

## Threat model

| Threat | Mitigation |
| --- | --- |
| Man in the middle on first connection | The user sees and confirms the fingerprint (OpenSSH's prompt); Subak never answers it. |
| Changed host key | OpenSSH refuses; the saved secret is never offered (ssh stops before authentication). |
| Secret on disk in plain text | Only Credential Manager (DPAPI); tests scan the config, state and test folders, `info`/`ls` JSON and the screens for the secret. |
| Secret disclosed to another program | The askpass helper prints the secret only to the ssh process that asked, once. Programs of the same user can read that user's Credential Manager anyway; Subak adds no wider exposure. |
| Wrong or expired secret replayed | Once per ssh process; ssh then reports `Permission denied`. |
| Local command execution via options | Options that run commands or weaken checks are refused. |
| Agent hijack on the remote side | Agent forwarding off. |

## Testing

`crates/subak/tests/ssh_sessions.rs` runs Microsoft's portable sshd
(`scripts/fetch-test-sshd.ps1`: pinned release, SHA-256 checked, into `target/test-sshd`,
nothing installed) as the user on 127.0.0.1 with a throwaway host key, and covers first trust,
a saved passphrase, a wrong one, a changed host key, the server offline, SSH profiles and the
leak scan. **Not tested:** password login (it would need a Windows account password),
keyboard-interactive/2FA, and the Windows ssh-agent (its service is disabled on the development
PC and enabling it needs an administrator).

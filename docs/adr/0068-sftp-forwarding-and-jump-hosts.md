# ADR 0068 — SFTP transfers, jump hosts, port forwarding, proxies and login scripts

- **Status:** Accepted (Step 35, 2026-11-11)
- **Requirements:** `SUBAK-FR-061`, `KIM-SEC-013`, `KIM-FR-013`, `KIM-FR-012`
- **Builds on:** [ADR 0067](0067-ssh-through-windows-openssh-and-credential-manager.md) (SSH
  through Windows' OpenSSH client, secrets in Credential Manager),
  [ADR 0065](0065-tabby-parity-priorities-and-ssh-transport-criteria.md)

## Decision

1. **SFTP is Subak's own SFTP version 3 client over `ssh.exe -s <host> sftp`.** Windows' OpenSSH
   client still does the connection, encryption, host keys, keys, agent and the saved secret;
   Subak only speaks the file protocol over its pipes (`crates/subak/src/sftp.rs`, no new crate).
   Driving `sftp.exe` was tried first: it shows progress only on a console, ignores errors in
   non-batch mode, and parsing its text would be fragile. Owning the protocol gives exact
   progress, cancellation between 32 KiB chunks (16 in flight), resuming from an offset, and file
   names that never pass through a shell.
2. **Transfers never trust and never ask.** `sftp` runs ssh with `StrictHostKeyChecking=yes`
   (a host not in known_hosts fails with `ssh_failed` and the hint to connect once with
   `subak ssh connect`), `ClearAllForwardings=yes`, `ForwardX11=no`, a connect timeout and
   keepalives (a dead link fails within ~30 s), and always the askpass helper: it answers a saved
   secret of a verified host or refuses, so ssh never reads a console (`KIM-FR-012`).
3. **Safe writes.** Every file is written as `<name>.subak-part` and renamed when complete
   (locally `ReplaceFileW`, remotely OpenSSH's `posix-rename`). An existing destination is replaced
   only with `--overwrite` (else `file_exists`, checked for every file before the first byte);
   `--resume` continues a partial file from its size. Remote names are checked before anything is
   written locally: `.`/`..`, `/`, `\`, `:`, wildcards, control characters, Windows device names
   (`CON`, `NUL`, `COM1`…, with any extension) and trailing dots or spaces are `unsafe_path`.
   Directory copies never follow links or junctions (they are listed as `skipped`). Uploads are
   created `0644`/`0755`, `--private` `0600`/`0700` (servers may apply their umask; Windows servers
   ignore modes).
4. **Background jobs are files, not a daemon.** `--background` starts the same command detached
   (no window, no inherited handle, out of the caller's job when allowed); it reports to
   `%APPDATA%\mouv\subak\transfers\<id>.json` (target, paths, state, progress, error — never a
   secret). `transfer cancel` drops a `<id>.cancel` file that the worker checks between chunks; a
   worker that died is reported `interrupted`. Finished records older than a week are removed.
5. **Jump hosts** are OpenSSH's `ProxyJump` (`--jump`, repeatable, profile `jump = [...]`). The
   jump ssh reads only the `-F` config (`--ssh-config`), which OpenSSH passes on. A saved secret
   of each hop is offered only when that hop is in known_hosts, and **a password prompt is
   answered only with the entry whose `user@host` the prompt names** — a jump host can never learn
   the destination's password (a key passphrase unlocks a local key, so the destination's entry
   answers it). Each entry is answered once per ssh process.
6. **Port forwards** (`--forward L:…`, `R:…`, `D:…`, profile `forwards`) listen on `127.0.0.1`
   unless a bind address is given; a bind beyond loopback needs `--allow-public-bind` on the
   command line (never from a profile) and is warned about. `ExitOnForwardFailure=yes` makes a
   failed forward end the connection visibly; a taken local port is `port_in_use` before anything
   starts. `--no-shell` (`ssh -N`) holds forwards in a pane without a remote command; closing the
   pane ends them. Forwards are fixed per connection: adding or removing them on a live session
   is not offered (Step 36).
7. **Agent forwarding and X11** are off unless asked (`--agent-forward`, profile
   `agent_forward = true`; `--x11` as untrusted `ForwardX11`), and every such connection carries
   a warning in `data.ssh.warnings` and on stderr. X11 needs an X server the user installed and
   `DISPLAY`; without it the connection is refused (`invalid_state`). Subak installs none.
8. **Proxies** (`--proxy socks5://host:port` or `http://host:port`, CONNECT) are relayed by
   `subak.exe __proxy-relay` as the `ProxyCommand` Subak itself writes (user `ProxyCommand`
   options stay refused). No proxy credentials. A proxy and jump hosts together are refused.
9. **Login scripts** (`--login-script <file.toml>`, profile `login_script`): `[[step]] expect =
   "…" send = "…" [enter] [timeout]` — Subak waits for a new occurrence of `expect` on the pane's
   screen or history, then types `send`. The result lists every step (what matched, what was
   sent, how long it waited); a step that times out stops the script and is warned about. **A
   script never holds a secret**: secret-looking text is refused (`config_invalid`); a saved
   password answers ssh's own prompt through the askpass helper instead.
10. **Not implemented** (kept `planned` in the Tabby matrix): Zmodem (needs the Host to detect
    `rz`/`sz` in the pane stream and the window to pick files; no fixture server has `lrzsz`), the
    SFTP panel in the window, drag and drop, editing a remote file locally, connection reuse
    (Windows' OpenSSH has no `ControlMaster`), the remote cwd for new panes, opening WinSCP, and
    managing forwards on a live session.

## Threat model (additions to ADR 0067)

| Threat | Mitigation |
| --- | --- |
| A malicious server lists `..\..\x`, `C:x` or `NUL` to write outside the target folder | Names checked before any local write (`unsafe_path`); unit test with a hostile in-process SFTP server. |
| A jump host learns the destination's password | Password prompts answered only by the entry the prompt names; unit-tested. |
| A transfer silently trusts a new or changed host | `StrictHostKeyChecking=yes`: unknown is `ssh_failed`; trust is given only interactively. |
| A forward exposed to the network | Loopback by default; public binds need an explicit flag and are warned about. |
| Agent hijack via forwarding | Off by default; explicit flag; warning naming the host. |
| A secret in a login script or job file | Secret-looking script text refused; job records hold paths and counts only. |
| A partial file mistaken for a complete one | `*.subak-part` until the last byte is written and synced; then renamed. |

## Testing

`crates/subak/tests/ssh_transfers.rs` runs three portable sshd servers as the user on
127.0.0.1 **on a private desktop of the test process** (the portable sshd starts `cmd.exe /c
sftp-server.exe` with a new console window for every SFTP session; on the private desktop none
reaches the user's screen — found in Step 35 when such windows appeared): an unknown host,
listing, a folder tree up and down, a 64 MiB file byte for byte, overwrite protection, a
cancelled upload resumed, a server killed mid-download (`ssh_failed`) and resumed, delete
confirmation, one and two jump hosts, L/R/D forwards, a taken port, SOCKS5 and HTTP proxies,
agent/X11 guards, a login script, refused options. A debug-only `SUBAK_TEST_SFTP_DELAY_MS`
slows chunks so a cancel or a dropped connection lands mid-transfer. **Not tested:** X11
forwarding with a real X server, agent forwarding with a running agent (the service is disabled
on the development PC), password login through a jump host, servers other than Win32-OpenSSH.

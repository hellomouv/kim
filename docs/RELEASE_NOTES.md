# Release notes

## Kim 0.0.1 and Subak 0.0.1 — preview (not yet published)

- **Status:** draft (Step 40, 2026-11-12). Nothing is published until the blockers of
  [RELEASE_READINESS §3](RELEASE_READINESS.md#3-blockers-and-known-limitations) are cleared;
  packages can be built with `cargo xtask package` ([RELEASING](RELEASING.md)).
- **Platform:** Windows 10 22H2 x64 tested; Windows 11 x64 not tested; no ARM64, no Linux yet.

### Kim 0.0.1

- A signed catalog of Kim tools (Ed25519, rollback protection) and an explicit installer:
  `install`, `update` (side by side; never stops running programs), **`rollback`** (new),
  `uninstall` (keeps user data unless `--purge-data`), all with `--dry-run` and `--json`.
- Never installs or updates by itself; offline mode; no telemetry.
- Tool discovery and capability interop: `tools list|describe|find|invoke`
  ([INTEROP](INTEROP.md)).
- Kim Manager window; Kim's own shortcuts (`hotkeys`).

### Subak 0.0.1

- A detached per-user Host owning Terminals, windows and panes (ConPTY); CLI and window
  clients; Terminals survive closing every window ([SUBAK_CLI](SUBAK_CLI.md), [SUBAK_GUI](SUBAK_GUI.md)).
- tmux-style commands, prefix keys, layouts, buffers, hooks ([TMUX_COMPATIBILITY](TMUX_COMPATIBILITY.md)).
- Claude Code status per pane from its hooks; crash recovery and cold restore with Claude resume
  ([RECOVERY](RECOVERY.md)).
- SSH through Windows' OpenSSH with secrets in Credential Manager, SFTP with jobs and resume,
  jump hosts, forwards, proxies; Telnet (off by default) and serial panes.
- An MCP server for agents, read-only unless granted ([MCP_GUIDE](MCP_GUIDE.md)).

### Known limitations

[RELEASE_READINESS §3](RELEASE_READINESS.md#3-blockers-and-known-limitations), the
[Tabby parity report](TABBY_PARITY_REPORT.md) (no full Tabby parity; follow-up #98) and the
unsupported rows of [TMUX_COMPATIBILITY](TMUX_COMPATIBILITY.md). Claude Code integration reads
Claude's hooks; Claude features beyond status and resume are not provided.

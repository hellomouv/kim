# Release readiness (Step 39)

- **Status:** current (Step 39, 2026-11-12). **Verdict: not ready for a public release** — the
  blockers of §3 remain. Everything below is evidence from this repository's tests, the
  benchmark ([PERFORMANCE](PERFORMANCE.md)) and the threat model ([SECURITY](SECURITY.md)); the
  publishing procedure is [RELEASE_POLICY](RELEASE_POLICY.md), and Step 40 prepares the release.
- Verified on Windows 10 Pro 10.0.19045 only. **Linux: not built, not tested** (Steps 41–42).

## 1. Checklist

| # | Item | State | Evidence |
| --- | --- | --- | --- |
| 1 | No install, update or network call without an explicit command | ✅ | `crates/kim/tests/cli_install.rs`, `catalog_tools.rs`, bench `host_sockets = 0`; SECURITY §5, §7 |
| 2 | No telemetry, no account, no cloud service | ✅ | ADR 0012; no network code outside the commands listed in SECURITY §7 |
| 3 | Secrets never in logs, JSON, state, config, IPC | ✅ | leak scans in `ssh_sessions.rs`, `mcp_server.rs`, `scrollback_replay.rs`, `state_persistence.rs`; `redact` tests |
| 4 | Catalog signatures, package integrity, rollback, atomic install | ✅ (dev key) | `crates/kim/tests/install.rs`, `cli_functional.rs` |
| 5 | Release signing key ceremony; Authenticode signatures | ❌ | Step 40 (RELEASE_POLICY §signing keys) |
| 6 | Host survives closing every client, detach, GUI crash | ✅ | `session_lifecycle.rs`, `milestone_a.rs`, `gui_window.rs` |
| 7 | Crash recovery without data loss or duplicates | ✅ | `milestone_c.rs`, `recovery.rs`, `state_persistence.rs` |
| 8 | Lock, sleep, hibernate, reboot, power loss | ⏳ maintainer | #31, `scripts/recovery-matrix.ps1` |
| 9 | Malformed IPC and MCP input | ✅ | `host_ipc.rs::fuzzed_frames_never_crash_the_host`, `proto.rs` fuzz, bench `mcp_malformed_answered` |
| 10 | Permission boundaries (other users, client kinds, MCP grants, interop consent) | ✅ | `host_ipc.rs`, `proto.rs`, `mcp_server.rs`, `crates/kim/tests/interop.rs` |
| 11 | Credential failures (wrong, expired, unknown host, changed key) | ✅ | `ssh_sessions.rs`, `ssh_transfers.rs` |
| 12 | Multi-session, multi-client concurrency | ✅ | `subak_cli.rs::concurrent_commands_share_one_host_and_never_duplicate_a_name`, `host_ipc.rs`, bench (10 Terminals, 10 panes) |
| 13 | Measured performance baseline and budgets | ✅ | `docs/perf/baseline.json`, `cargo xtask bench --check` |
| 14 | No leaks in Subak's own code over create/kill and capture cycles | ✅ (see §3 for the OS's) | bench soak; `sessions_hold_no_handle_beyond_the_one_the_inbox_conpty_keeps` |
| 15 | Idle cost zero | ✅ | bench: 0 ms CPU / 10 s, 0 state writes, 0 sockets |
| 16 | High DPI | ✅ per-monitor v2 | `subak-gui` and Noeul are per-monitor-DPI-aware v2 and handle `WM_DPICHANGED`; `gui_float.rs` DPI 192 case; Noeul display tests |
| 17 | Accessibility | ◐ | system controls and accessible names in the floating manager (`gui_float.rs`), Claude states in words not colour only, High Contrast theme; **Narrator pass pending — maintainer** (SUBAK_GUI) |
| 18 | Offline mode | ✅ | Kim `offline_mode` refuses network commands (`offline_mode`, exit 6); every other tool works offline |
| 19 | e2e suite daily | ✅ | `Daily` workflow; `e2e/LEDGER.md` |
| 20 | Visible-window and real-device checks of earlier Steps | ⏳ maintainer | open issues #8, #14–#20, #55–#58, #61–#64, #71, #73–#80 (DEV_CHECKPOINT) |
| 21 | Third-party security review | ❌ | not done |

## 2. Release rollback

Kim keeps the previous version's files until the next update and restores them when an install
fails at its commit point (`cli_functional.rs::a_protected_install_record_aborts_the_update_at_the_commit_point_and_rolls_back`).
A user-initiated downgrade is an explicit `kim install <tool> --version <older>`; an automatic
rollback command is Step 40 (RELEASE_POLICY).

## 3. Blockers and known limitations

| Item | Kind | Status |
| --- | --- | --- |
| Release signing key and Authenticode signing | **Blocker** | Step 40 |
| Power-transition matrix (#31) | **Blocker** for the recovery claim | maintainer runs it |
| Visible-window/real-device checks of earlier Steps | **Blocker** for their features | the open maintainer-check issues |
| Windows 10 inbox ConPTY keeps one exited-conhost handle per pane | Known limitation (measured: 1 per pane; released when the Host exits) | documented (PERFORMANCE §3); a regression test guards Subak's own code |
| Redaction is heuristic | Known limitation | documented (SECURITY §2, §6) |
| Serial hardware, X11, agent forwarding, non-Win32 SSH servers, MCP clients other than Claude Code | Not tested | listed in each ADR |
| Linux | Not built | Steps 41–42 |

## 4. Evidence of this Step

`cargo xtask ci` green; `cargo xtask bench --record` (the baseline) and `--check` pass on the
development PC; the ConPTY handle tests of `crates/subak/src/win/conpty.rs`; e2e `step39` runs the
`subak host stats` path the bench reads.

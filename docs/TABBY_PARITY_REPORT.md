# Tabby parity report (Step 36)

- **Status:** current (Step 36, 2026-11-11). Source of truth: [TABBY_FEATURE_MATRIX](TABBY_FEATURE_MATRIX.md); decisions: [ADR 0065](adr/0065-tabby-parity-priorities-and-ssh-transport-criteria.md), [ADR 0067](adr/0067-ssh-through-windows-openssh-and-credential-manager.md), [ADR 0068](adr/0068-sftp-forwarding-and-jump-hosts.md), [ADR 0069](adr/0069-telnet-serial-and-the-parity-review.md).
- **Baseline:** Tabby v1.0.238 (2026-10-07), as audited in Step 32.
- **Verdict: Subak does not have full Tabby parity.** It has a different core (a detached Host, a headless CLI, tmux-style windows and panes, Claude Code status) and covers Tabby's SSH, SFTP, Telnet and serial basics headlessly; most of Tabby's window conveniences and its SFTP panel are not built yet and are tracked in follow-up #98.

## 1. Totals

| Status | Rows |
| --- | --- |
| `implemented` | 16 |
| `partial` | 83 |
| `planned` | 79 |
| `excluded` | 27 |
| `blocked` | 4 |
| **Total** | **209** |

By area (prefix of the row ID):

| Area | implemented | partial | planned | excluded | blocked |
| --- | --- | --- | --- | --- | --- |
| `APP` | 0 | 3 | 5 | 6 | 1 |
| `CFG` | 0 | 3 | 3 | 2 | 0 |
| `CORE` | 5 | 42 | 31 | 3 | 2 |
| `PLUG` | 0 | 0 | 4 | 5 | 0 |
| `SER` | 0 | 6 | 8 | 0 | 0 |
| `SHELL` | 3 | 8 | 13 | 2 | 1 |
| `SSH` | 4 | 17 | 8 | 2 | 0 |
| `TEL` | 2 | 1 | 1 | 0 | 0 |
| `VAULT` | 2 | 0 | 2 | 4 | 0 |
| `WEB` | 0 | 0 | 0 | 3 | 0 |
| `XFER` | 0 | 3 | 4 | 0 | 0 |

## 2. Every `implemented` row and its evidence

Each test named here ran green in this Step's `cargo xtask ci` (Windows 10 Pro 10.0.19045); `docs-check` verifies that every evidence path and test name exists.

| Row | Feature | Evidence |
| --- | --- | --- |
| `CORE-04` | Fast output handling | T: `crates/subak/tests/milestone_a.rs::output_bursts_and_resize_storms_never_block_or_end_anything`; T: `crates/subak/tests/scrollback_replay.rs::a_stalled_client_during_sustained_output_is_resynced_to_the_same_screen` (Step 32 audit) |
| `CORE-05` | Scrollback size | T: `crates/subak/tests/scrollback_replay.rs::history_is_bounded_evicts_the_oldest_lines_and_clears_on_request` (Step 32 audit) |
| `CORE-26` | Nested split panes | T: `crates/subak/tests/pane_layout.rs::splits_resize_zoom_swap_and_close_keep_processes_and_real_pty_sizes`; T: `crates/subak/tests/tmux_journeys.rs::a_long_running_program_survives_every_multiplexer_journey` (Step 32 audit) |
| `CORE-34` | Remember split layout and cwd | T: `crates/subak/tests/state_persistence.rs::topology_survives_a_clean_stop_and_loading_it_starts_nothing`; T: `crates/subak/tests/recovery.rs::a_saved_terminal_comes_back_as_it_was_without_rerunning_anything` (Step 32 audit) |
| `CORE-54` | Several bindings per action | T: `crates/subak/tests/hotkeys_cli.rs::list_show_and_check_report_the_shipped_defaults_without_a_host_or_a_file` (Step 32 audit) |
| `SHELL-01` | Windows PowerShell 5.1 | T: `crates/subak/tests/profiles_integration.rs::detected_and_configured_profiles_start_real_shells_and_missing_ones_fail_clearly`; T: `crates/subak/src/shells.rs::tests::detection_finds_this_pcs_shells_without_running_anything` (Step 33) |
| `SHELL-03` | CMD | T: `crates/subak/tests/profiles_integration.rs::detected_and_configured_profiles_start_real_shells_and_missing_ones_fail_clearly`; T: `crates/subak/src/shells.rs::tests::detection_finds_this_pcs_shells_without_running_anything` (Step 33) |
| `SHELL-07` | Git Bash | T: `crates/subak/tests/profiles_integration.rs::detected_and_configured_profiles_start_real_shells_and_missing_ones_fail_clearly`; T: `crates/subak/src/shells.rs::tests::detection_finds_this_pcs_shells_without_running_anything` (Step 33) |
| `SSH-01` | SSH2 client | T: `crates/subak/tests/ssh_sessions.rs::ssh_sessions_trust_hosts_explicitly_and_keep_secrets_in_credential_manager`; T: `crates/subak/src/ssh.rs::tests::the_plan_is_strict_and_uses_a_secret_only_for_a_verified_host` (Step 34) |
| `SSH-10` | Host key verification, known hosts | T: `crates/subak/tests/ssh_sessions.rs::ssh_sessions_trust_hosts_explicitly_and_keep_secrets_in_credential_manager`; T: `crates/subak/src/ssh.rs::tests::options_that_run_commands_or_weaken_checks_are_refused` (Step 34) |
| `SSH-17` | Jump hosts | T: `crates/subak/tests/ssh_transfers.rs::sftp_forwarding_and_jump_hosts_work_against_real_servers`; T: `crates/subak/src/ssh.rs::tests::a_saved_password_answers_only_the_host_it_was_saved_for`; T: `crates/subak/src/ssh.rs::tests::jumps_forwards_and_agent_forwarding_reach_ssh_as_asked` (Step 35) |
| `SSH-21` | Dynamic (SOCKS) forwarding | T: `crates/subak/tests/ssh_transfers.rs::sftp_forwarding_and_jump_hosts_work_against_real_servers`; T: `crates/subak/src/ssh.rs::tests::forwards_bind_loopback_unless_told_otherwise` (Step 35) |
| `TEL-01` | Telnet client | T: `crates/subak/tests/telnet_serial.rs::telnet_is_opt_in_negotiates_safely_and_serial_reports_a_missing_port` (Step 36) |
| `TEL-02` | Option negotiation | T: `crates/subak/tests/telnet_serial.rs::telnet_is_opt_in_negotiates_safely_and_serial_reports_a_missing_port`; T: `crates/subak/src/remote.rs::tests::telnet_negotiates_only_what_a_terminal_needs` (Step 36) |
| `VAULT-01` | Encrypted container for SSH secrets | T: `crates/subak/src/ssh.rs::tests::credential_manager_round_trip`; T: `crates/subak/tests/ssh_sessions.rs::ssh_sessions_trust_hosts_explicitly_and_keep_secrets_in_credential_manager` (Step 34) |
| `VAULT-02` | OS keychain when the vault is off | T: `crates/subak/tests/ssh_sessions.rs::ssh_sessions_trust_hosts_explicitly_and_keep_secrets_in_credential_manager` (Step 34) |

## 3. Intentional deviations (`excluded`) and blockers

| Row | Feature | Decision (from the matrix) |
| --- | --- | --- |
| `CORE-06` (blocked) | Sixel inline images | Blocked: older inbox ConPTY re-renders output and may drop Sixel data. Step 15 kept the inbox ConPTY for M2 ([ADR 0036](adr/0036-terminal-emulation-and-rendering-path.md) §5: on 19045 even mouse modes and OSC 8 do not pass) and the emulator does not decode images; unblocked by shipping a newer ConPTY and an image-capable renderer |
| `CORE-43` (blocked) | Font ligatures | Shaped in the renderer. Off by default, as in Tabby. *Blocked (Step 20, ADR 0051 §7):* the GDI renderer draws each cell on its own with glyph indices; ligatures need shaping across cells (a DirectWrite or Uniscribe rasterizer behind the same `Rasterizer` boundary). The settings menu says so |
| `CORE-50` (excluded) | Background image | Excluded: it hurts legibility and the restrained Kim look (KIM-NFR-012). Revisit at Step 32. |
| `CORE-51` (excluded) | Custom CSS | Excluded: there is no web renderer. Styling goes through theme tokens and scheme files. |
| `CORE-77` (excluded) | Search selected text in the browser | *Rationale:* it sends terminal output to a web search service; Subak never sends output to a network service (SUBAK-SEC-014, ADR 0048). Copy the text and search yourself |
| `SHELL-05` (excluded) | Bundled Clink | Excluded: injects a bundled third-party DLL into `cmd.exe` and adds an update surface Subak does not control ([SECURITY.md](../SECURITY.md)). Use `SHELL-04` instead. |
| `SHELL-22` (blocked) | Run as administrator | Blocked: bridging an elevated pane into the non-elevated per-user Host crosses an elevation boundary, so it needs a security ADR first. Never silent (KIM-SEC-004). |
| `SHELL-25` (excluded) | Legacy WinPTY backend | Excluded: ConPTY only (SUBAK-FR-016). It is always present on Windows 10 1809+ (KIM-NFR-010). |
| `SSH-30` (excluded) | Key names instead of paths for synced profiles | Excluded: it exists to make cloud-synced profiles portable. Subak has no sync (`CFG-04`), and key paths stay local. |
| `SSH-31` (excluded) | In-app web authentication pop-ups | Excluded: no embedded browser (Kim philosophy §5). Auth URLs open in the system browser through `CORE-74`. |
| `VAULT-03` (excluded) | Vault master passphrase | Excluded: protection follows the Windows logon (DPAPI), so there is no extra passphrase file to lose or brute-force. Re-authentication before revealing a secret is `VAULT-05`. |
| `VAULT-04` (excluded) | Encrypted configuration | Excluded: the config contains no secrets (KIM-SEC-013) and stays human-readable (SUBAK-FR-046) |
| `VAULT-07` (excluded) | Plugin secrets in the vault | Excluded: there are no plugins. Each Kim tool uses its own Credential Manager namespace. |
| `VAULT-08` (excluded) | Auto-typed sudo password | Excluded: typing a stored password whenever output matches a prompt lets any remote program fake the prompt and collect it |
| `CFG-04` (excluded) | Cloud config sync | Excluded: no accounts and no cloud service (KIM-SEC-010) |
| `CFG-05` (excluded) | Sync to Gist or Gitee | Excluded: relies on a third-party cloud. Users can keep `config.toml` in a folder they sync themselves (local-only). |
| `PLUG-01` (excluded) | Plugin manager | Excluded: there is no npm runtime inside Subak. Extra capabilities ship as independent Kim tools, installed only by explicit action (KIM-FR-001, KIM-FR-020, KIM-SEC-001). |
| `PLUG-02` (excluded) | Plugin API | Excluded: replaced by the documented CLI/JSON contract and the local IPC (KIM-FR-010, SUBAK-SEC-012) |
| `PLUG-03` (excluded) | Plugin, command and provider blocklists | Excluded: there are no plugins to block (`PLUG-01`). Built-in features have their own switches (e.g. Telnet). |
| `PLUG-04` (excluded) | Installable theme packages | Excluded: themes as code packages need the plugin runtime (`PLUG-01`). Equivalent looks ship as data (`PLUG-05`). |
| `PLUG-09` (excluded) | Example plugin | Excluded: a demo of the plugin API (`PLUG-02`) |
| `WEB-01` (excluded) | Web app | Excluded: Subak is a local-first native app, with no browser client and no account (KIM-SEC-010) |
| `WEB-02` (excluded) | Self-hosted web server | Excluded: it would need a network listener. Subak's IPC is local and same-user only (SUBAK-SEC-010). |
| `WEB-03` (excluded) | Web demo | Excluded: it demonstrates the web app (`WEB-01`) |
| `APP-02` (excluded) | Automatic updates | Excluded: nothing updates by itself. Updates happen only through an explicit Kim action (KIM-SEC-001, KIM-SEC-002). |
| `APP-03` (excluded) | Usage analytics | Excluded: no tracking and no telemetry (KIM-SEC-011, KIM-SEC-012) |
| `APP-10` (excluded) | Safe mode | Excluded: there are no plugins to disable, and a broken config already falls back to defaults (SUBAK-FR-046) |
| `APP-11` (excluded) | Electron flags and DevTools | Excluded: there is no Electron. Diagnostics are local logs that contain no terminal content (KIM-SEC-012). |
| `APP-12` (excluded) | Debug state and output hotkeys | Excluded: developer tooling. Capture with JSON output covers inspection (SUBAK-FR-012). |
| `APP-13` (blocked) | Windows ARM64 build | Blocked: no ARM64 test machine yet, and untested platforms are not claimed (Product requirements §2) |
| `APP-15` (excluded) | macOS app (Touch Bar, Dock menu) | Excluded: macOS is out of scope (Product requirements §2) |

## 4. Partial rows and the follow-up backlog

83 rows are `partial`: the matrix says per row what works, what is missing and where it goes. 79 rows are `planned`; those not owned by a later numbered Step (37–44) belong to a group of **#98**:

| Group | Scope |
| --- | --- |
| T1 | Window, tabs and panes |
| T2 | Keyboard and line editing |
| T3 | Notifications, activity and the dropdown window |
| T4 | Shells, profiles, settings and themes |
| T5 | SSH, SFTP panel, Zmodem and secrets |
| T6 | Serial and Telnet extras |

## 5. Validation across terminal kinds (Step 36)

| Kind | What was checked | Evidence |
| --- | --- | --- |
| Local shells | Every Step 10–33 scenario (tabs, panes, tmux commands, detach, recovery, Claude status) | `cargo xtask ci`: all subak tests green |
| SSH | First trust, saved secrets, changed host key, offline server, profiles | `crates/subak/tests/ssh_sessions.rs` |
| SFTP and forwarding | Transfers, cancel/resume, dropped connection, jumps, L/R/D, proxies | `crates/subak/tests/ssh_transfers.rs` |
| Telnet | Off by default, negotiation, a login exchange, the server hanging up | `crates/subak/tests/telnet_serial.rs` |
| Serial | Port list, settings, a missing port reported in the pane; **no real port tested** | `crates/subak/tests/telnet_serial.rs` |

**Resource use** (release build, idle 10 s, Host with 8 shell panes and 1 Telnet pane; `Get-Process`):
Host 0 ms CPU, 7.5 MiB working set, 3.3 MiB private; Telnet bridge 0 ms CPU, 5.3 MiB working set,
1.0 MiB private. A bridge process exists only while its pane is open; the Host itself runs no
device or network loop.

**Recovery boundaries:** SSH, Telnet and serial panes are Host-owned and survive closing the window
and detaching like any Terminal. After a reboot they are saved Terminals whose start command is
re-run only with `subak recover --with-commands` (never automatically, `SUBAK-SEC-003`);
disconnected tabs with a Reconnect action are #98 T5 (`CORE-35`). Claude Code status is unaffected
by remote panes (it follows Claude's hooks inside a pane).

## 6. Safety summary

- No remote service is started by default: Subak listens only on its per-user named pipe; a
  forward listens only when the user asks, on loopback unless `--allow-public-bind`.
- Telnet needs `--allow-insecure` and always warns; agent and X11 forwarding are off unless asked
  and always warn.
- Secrets: only Windows Credential Manager; never in config, state, logs, JSON, login scripts or
  transfer jobs (tests scan for them).
- Host keys: never trusted silently; transfers refuse unknown hosts.
- The Host stays per-user and on-demand (no service, no always-on daemon).


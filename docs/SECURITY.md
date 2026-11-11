# Security model and threat analysis (Step 39)

- **Status:** current (Step 39, 2026-11-12). Reporting vulnerabilities and the supply-chain
  policy: [../SECURITY.md](../SECURITY.md). Requirements: `KIM-SEC-*`, `SUBAK-SEC-*`,
  [PRODUCT_REQUIREMENTS](PRODUCT_REQUIREMENTS.md).
- **Trust boundary:** the interactive Windows user. Kim, Subak, Noeul and Ddalgak run as that
  user, never elevated, never as a service; another user, a remote machine and a web page are
  outside. Code already running as the same user can do what the user can (it could run the
  tools itself); the defences below keep the tools from widening that, from being steered by
  untrusted input, and from leaking what they see.

Each row: the threat, what stops it, the evidence (tests that fail if it regresses), and what
remains.

## 1. Local IPC (Subak Host)

| Threat | Mitigation | Evidence | Residual |
| --- | --- | --- | --- |
| Another user connects to the Host's pipe | Pipe DACL for the user only; the Host checks each client's token SID and refuses others (`unauthorized`) | `crates/subak/tests/host_ipc.rs` (identity), ADR 0019 | — |
| A pipe squatter impersonates the Host | Clients check the server pid's user and image name (`subak.exe`) before sending anything (`host_identity_mismatch`) | `host_ipc.rs`, `win/pipe.rs` tests | A same-user `subak.exe` copy is trusted (same trust level) |
| Malformed or hostile frames | Bounded frames refused from their header, fuzzed frames never crash the Host, a 64-connection limit | `host_ipc.rs::fuzzed_frames_never_crash_the_host`, `proto.rs` fuzz tests | — |
| A `hook` or `mcp` client acts | Method authorization by client kind: hooks report only; `mcp` reads only, screens redacted | `proto.rs::methods_are_authorized_by_client_kind`, `scrollback_replay.rs` | — |

## 2. MCP (`subak mcp serve`, ADR 0070)

| Threat | Mitigation | Evidence | Residual |
| --- | --- | --- | --- |
| A remote party reaches the server | stdio only; no listener | `mcp.rs` | — |
| A model runs or types something unwanted | No input/program tool without the user's `--allow input`/`run`; destructive tools preview until `confirm: true`; clients ask per call | `crates/subak/tests/mcp_server.rs` | A user who grants `input` grants what a shell can do |
| Secrets on screens reach the model | The Host redacts every capture for `mcp` connections | `mcp_server.rs`, `scrollback_replay.rs` | Redaction is heuristic (token shapes, secret names) |
| Malformed or huge messages | JSON-RPC errors for every bad line, 1 MiB message cap, limits per tool | `mcp.rs` tests, `cargo xtask bench` (`mcp_malformed_answered`) | — |
| Hidden actions | Audit trail without typed text | `mcp_server.rs` | The log is the user's to keep or delete |

## 3. Command input

| Threat | Mitigation | Evidence | Residual |
| --- | --- | --- | --- |
| Shell injection through names, targets or options | Programs start with `CreateProcessW` from an argv (or the user's verbatim `--command-line`); names are validated; no tool builds a shell string from untrusted input | `session.rs` name tests, `subak_cli.rs` verbatim tests | — |
| Options that run commands (ssh `ProxyCommand`, `LocalCommand`, `Match exec`) | Refused in `--option`; Subak writes its own ProxyCommand for `--proxy` | `ssh.rs::options_that_run_commands_or_weaken_checks_are_refused` | The user's own `~/.ssh/config` is trusted as theirs |
| Interop calls with a program of the caller's choice | `terminal.open` starts shells and profiles only; consent per effect in `kim tools invoke` | `interop_provider.rs`, `crates/kim/tests/interop.rs` | — |

## 4. SSH credentials and remote sessions (ADR 0067, 0068)

| Threat | Mitigation | Evidence | Residual |
| --- | --- | --- | --- |
| Secrets on disk or in output | Only Windows Credential Manager (DPAPI); never config, state, logs, JSON, IPC, login scripts or transfer jobs | `ssh_sessions.rs` leak scan, `ssh.rs::credential_manager_round_trip` | Same-user programs can read the user's Credential Manager |
| A host trusted silently | `StrictHostKeyChecking=ask` in panes (the user answers), `yes` for transfers | `ssh_sessions.rs`, `ssh_transfers.rs` | — |
| A jump host learns the destination's password | A saved password answers only the prompt naming its own `user@host` | `ssh.rs::a_saved_password_answers_only_the_host_it_was_saved_for` | — |
| A replayed wrong secret | Once per ssh process and entry | `ssh_sessions.rs` | — |
| A malicious SFTP server writes outside the folder | Remote names checked before any write; links never followed | `sftp.rs::a_hostile_listing_is_refused_before_anything_is_written` | — |
| Forwards exposed to the network | Loopback by default; public binds need a flag and warn | `ssh_transfers.rs` | — |
| Telnet sends secrets in clear text | Off by default, explicit `--allow-insecure`, always warned | `telnet_serial.rs` | A user who opts in accepts it |

## 5. Packages, signatures and updates (Kim, ADR 0013, [RELEASE_POLICY](RELEASE_POLICY.md))

| Threat | Mitigation | Evidence | Residual |
| --- | --- | --- | --- |
| Silent install or update | Nothing installs without an explicit command; `update check` never downloads; launch checks only display | `crates/kim/tests/cli_install.rs`, `catalog_tools.rs` | — |
| A tampered catalog or package | Ed25519-signed catalog, rollback protection, SHA-256 and size of every package, an atomic install with rollback | `crates/kim/tests/install.rs`, `cli_functional.rs` | **Release signing keys and Authenticode signing are not set up** (release checklist) |
| Elevation | Per-user locations only; never UAC | `install.rs` | — |
| Tool interop installing something | Interop never installs (`install` effect refused) | `crates/kim/tests/interop.rs` | — |

## 6. Terminal output capture and session restoration

| Threat | Mitigation | Evidence | Residual |
| --- | --- | --- | --- |
| Secrets in captures and history | Captures redacted by default (`--no-redact` for the user only); history never persisted | `scrollback_replay.rs` | Heuristic redaction |
| Restoration re-runs something | Restore starts fresh shells; start commands only with `--with-commands`; never kills a live session | `recovery.rs`, `milestone_c.rs` | — |
| A corrupt or newer `state.json` | CRC, backups, read-only for newer versions, never overwritten without a backup | `state_persistence.rs` | — |
| A crashed Host loses work | Terminals' state survives; cold recovery rebuilds without duplicates | `milestone_c.rs` | Power-loss/sleep matrix needs the maintainer (#31) |

## 7. Network, privacy and telemetry

No tool opens a listener, sends telemetry or contacts a network without an explicit command
naming it (`kim catalog refresh`, `kim install`, `ssh connect`, `sftp`, `telnet connect`). The
idle Host owns no socket (`cargo xtask bench`: `host_sockets = 0`). Kim's `offline_mode` refuses
every network operation (`offline_mode`, exit 6).

## 8. Open risks (release blockers or follow-ups)

See [RELEASE_READINESS §3](RELEASE_READINESS.md#3-blockers-and-known-limitations): unsigned
binaries and no release key ceremony yet; redaction is heuristic; the power-transition matrix
(#31) is manual; no third-party security review yet.

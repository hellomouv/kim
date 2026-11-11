# Development checkpoint

Sequential workflow ([ADR 0055](docs/adr/0055-single-branch-sequential-development.md)): one
session, one folder, one branch (`main`), one Step at a time. Update this file at every Step
transition. Resume prompt: *"Continue the Kim sequential development mission from the latest
checkpoint."*

## Now

- Branch: `main` · last pushed commit: see `git log -1 origin/main`
- Current Step: **Step 60** (#65) — Noeul capture history and pin backup recovery
- Last fully completed Step: Step 30 (#30); Step 31 (#31) done as far as possible, left OPEN:
  lock/sleep/hibernate/reboot/power rows need the maintainer (`scripts/recovery-matrix.ps1`)
- Uncommitted changes: none expected between Steps
- Next action: read #65 (Step 60) and its dependencies. Step 44 (#45) done (watermelon icon
  assets, per-Terminal Claude motion, ADR 0073). Step 43 (#44) Windows part done, Linux half open.
- Loop directive (user, 2026-11-11): keep implementing until `gh api --paginate
  repos/hellomouv/kim/issues?state=open` (no PRs) is 0; re-query after every issue

## Verification log (latest first)

- 2026-11-12: Step 44 — claude_motion unit tests 6/6, art/tabs frame tests, icon_assets 3/3
  (byte-exact assets, SVG colours, legibility 16–48 px on 4 surfaces), gui_window motion test
  (turn, blink, minimized 0 frames, static, settle, new window; 0.0 ms CPU over 10 s turning),
  gui_float badge test (no tray update per frame), e2e step44 1/1.

- 2026-11-12: Step 43 (Windows) — kim-setup unit tests 2/2, setup.rs 1/1 (install only Kim,
  refuse/repair/upgrade/downgrade, damaged installer, running refusal, uninstall keeping data;
  test registry key), e2e step43 1/1 with the release Kim programs; `xtask package kim` writes
  Kim-Setup-0.0.1-x86_64.exe (6.3 MB); the double-click window path is the maintainer's check.

- 2026-11-12: Step 40 — `cargo xtask package kim|subak` (deterministic, re-pack same SHA-256,
  notices), `kim rollback` (rollback.rs 1/1, contract goldens), `package --signed` refuses
  unsigned programs, e2e step40 1/1 (install → running → side-by-side update → rollback →
  offline → uninstall keeping data); release.yml not run (no certificate).

- 2026-11-12: Step 39 — `cargo xtask bench --record`/`--check` (22 measurements, budgets in
  docs/perf/baseline.json), found and pinned the Windows 10 inbox ConPTY handle per pane
  (existing conpty tests: Subak adds none), `subak host stats`, e2e step39 1/1; SECURITY,
  PERFORMANCE, RELEASE_READINESS docs.

- 2026-11-11: Step 38 — kim interop/redact unit tests, kim interop 1/1 (fixture: consent per
  effect, dry run, versions, missing/old/hung/garbage/failing provider, nothing installed),
  contract goldens (tools-find, tools-invoke), subak interop_provider 1/1, e2e step38 1/1
  (release kim → release subak).

- 2026-11-11: Step 37 — mcp unit tests (grants, annotations, handshake, refusals), mcp_server 1/1
  (read-only with no Host, redacted token, ungranted tools refused, cancelled wait, create/type/
  wait/split/keys, run required for a program, kill preview then confirm, audit without text),
  e2e step37 1/1, real Claude Code 2.1.322 `claude -p --mcp-config` called list_terminals.

- 2026-11-11: Step 36 — remote/ssh/config unit tests (Telnet state machine, serial newlines and
  DCB bits, ssh config hosts, profile groups), telnet_serial 1/1 (opt-in, negotiation, login,
  hang-up, missing COM port, ssh hosts), e2e step36 1/1; idle resources: Host 7.5 MiB / 0 ms CPU
  with 9 panes, Telnet bridge 5.3 MiB / 0 ms; parity report + follow-up #98; not tested: real
  serial hardware.

- 2026-11-11: Step 35 — ssh/sftp unit tests (hostile SFTP listing, per-host askpass, forwards,
  login scripts), ssh_transfers 1/1 against three real sshd servers (tree, 64 MiB, overwrite,
  cancel+resume, server killed+resume, jumps 1 and 2 hops, L/R/D, SOCKS5/HTTP proxy, port
  collision, guards), e2e step34+35 2/2 on a hidden desktop (no window); not tested: real X11,
  agent forwarding with a running agent.

- 2026-11-11: Step 34 — ssh unit tests 6/6, ssh_sessions 1/1 against a real local sshd (first
  trust, saved passphrase, wrong secret, changed key, offline, SSH profiles, leak scan), e2e
  step34 1/1 (focused local run, LEDGER); not tested: password login, 2FA, the ssh-agent.
- 2026-11-11: Step 33 — shells/integration/term unit tests, profiles_integration 2/2 (detected
  profiles, title, missing shell, PowerShell and cmd folder reports, state keeps it), e2e step33
  1/1 (focused local run, LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 32 — Tabby matrix audited (39 rows reclassified, checklist, ADR 0065),
  docs-check validates the matrix (209 rows, 83 evidence refs); `cargo xtask ci` before push.
- 2026-11-11: Step 31 — milestone_c 1/1 (Host crash → intact state → cold rebuild 99 ms, no
  re-run, no duplicate); recovery-matrix.ps1 smoke: check-live PASS, check-cold after a Host
  kill PASS (79 ms); power transitions NOT TESTED (maintainer).
- 2026-11-11: Step 30 — recovery 2/2 (topology, sizes, id, no rerun, confirm, duplicate, auto,
  Claude resume decisions), a real Claude 2.1.322 conversation resumed by `subak recover
  --claude-resume`, e2e step30 1/1 (focused local run, LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 29 — state unit tests 5/5, state_persistence 3/3 (clean stop, no relaunch,
  killed Host, torn file, newer version, rollback, secrets, private Hosts), e2e step29 1/1
  (focused local run, LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 28 — claude/tabs/model/float unit tests, claude_status 1/1 (all states,
  lost and stale signals, repeated states), gui_window claude tab test (off-screen), a real
  `claude -p` (2.1.322) with the 8-event hook set, e2e step27+28 2/2 (focused local run,
  LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 27 — claude unit tests 3/3, claude_hooks 2/2, a real `claude -p` (2.1.322)
  in a Subak pane reported SessionStart/UserPromptSubmit/Stop/SessionEnd, e2e step27 1/1
  (focused local run, LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 26 — xtask tmux matrix check (606 rows, 302 evidence refs), tmux_journeys
  1/1, e2e step26 1/1 (focused local run, LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 25 — layout/tmuxcmd/session/screen unit tests, tmux_advanced 2/2, full
  `cargo test -p subak -p subak-term` green, e2e step25 1/1 (focused local run, LEDGER);
  `cargo xtask ci` before push.
- 2026-11-11: Step 24 — tmuxcmd unit tests, tmux_commands 2/2, gui prompt/copy-mode test, e2e
  step24 2/2 (focused local run, LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 23 — keys unit tests, gui prefix test, CLI detach keys test, e2e step23
  2/2 (focused local run, LEDGER); `cargo xtask ci` before push.
- 2026-11-11: Step 22 — `cargo xtask ci` green (after fixing hotkey scenario 13); pane_layout 5/5,
  gui split test, e2e step22 2/2 (focused local run, LEDGER).
- 2026-11-11: Daily e2e had failed 7× (stale `step17`/`step18` scenarios); fixed, focused local
  run 4/4 PASS; recorded in `e2e/LEDGER.md`.

## Queue (Step number in the issue title; reconstruct with
`gh issue list --repo hellomouv/kim --state open --limit 200`)

1. Subak: 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 43
   (Windows part), 44
2. Noeul: 60, 61, 62, 63
3. Ddalgak: 76, 77, 78, 79, 80, 81, 82

## Blocked

- **Step 31 (#31)**: Milestone C needs real lock/unlock, sleep, hibernate, reboot and power-loss
  runs by the maintainer (rows 10–16 of MILESTONE_C_RECOVERY_VALIDATION); everything automatable
  passes.
- **Linux Steps 41, 42, 64, 83 and the Linux half of 43 (#44)**: AGENTS.md §3 forbids Linux work on
  this Windows PC (maintainer decision 2026-11-04); they wait for the Linux server phase.
- **Maintainer manual checks** (implemented and merged; issues stay open until the visible-window /
  real-input / real-clipboard / multi-monitor checks named in each issue's report are done):
  #8, #14, #15, #16, #18, #19, #20, #55–#58, #61–#64, #71, #73–#80.

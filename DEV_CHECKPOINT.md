# Development checkpoint

Sequential workflow ([ADR 0055](docs/adr/0055-single-branch-sequential-development.md)): one
session, one folder, one branch (`main`), one Step at a time. Update this file at every Step
transition. Resume prompt: *"Continue the Kim sequential development mission from the latest
checkpoint."*

## Now

- Branch: `main` · last pushed commit: see `git log -1 origin/main`
- Current Step: **Step 33** (#33) — shell profiles and productivity tools
- Last fully completed Step: Step 30 (#30); Step 31 (#31) done as far as possible, left OPEN:
  lock/sleep/hibernate/reboot/power rows need the maintainer (`scripts/recovery-matrix.ps1`)
- Uncommitted changes: none expected between Steps
- Next action: read #33 (the Tabby matrix checklist lists its 37 rows; ADR 0065 priorities)
- Loop directive (user, 2026-11-11): keep implementing until `gh api --paginate
  repos/hellomouv/kim/issues?state=open` (no PRs) is 0; re-query after every issue

## Verification log (latest first)

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
- **Linux Steps 41, 42, 64, 83 and the Linux half of 43**: AGENTS.md §3 forbids Linux work on
  this Windows PC (maintainer decision 2026-11-04); they wait for the Linux server phase.
- **Maintainer manual checks** (implemented and merged; issues stay open until the visible-window /
  real-input / real-clipboard / multi-monitor checks named in each issue's report are done):
  #8, #14, #15, #16, #18, #19, #20, #55–#58, #61–#64, #71, #73–#80.

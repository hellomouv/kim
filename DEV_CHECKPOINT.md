# Development checkpoint

Sequential workflow ([ADR 0055](docs/adr/0055-single-branch-sequential-development.md)): one
session, one folder, one branch (`main`), one Step at a time. Update this file at every Step
transition. Resume prompt: *"Continue the Kim sequential development mission from the latest
checkpoint."*

## Now

- Branch: `main` · last pushed commit: see `git log -1 origin/main`
- Current Step: **Step 25** (#25) — advanced tmux behaviors
- Last fully completed Step: Step 24 (#24, tmux commands, copy mode, buffers, ADR 0058)
- Uncommitted changes: none expected between Steps
- Next action: read #25; many rows of TMUX_COMPATIBILITY moved to Step 25 with a rationale
  (layout presets, last-window/last-pane, marked panes, key tables, formats, copy-mode extras,
  bracketed paste) — pick what #25 asks for and keep the rest honest

## Verification log (latest first)

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

1. Subak: 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 43
   (Windows part), 44
2. Noeul: 60, 61, 62, 63
3. Ddalgak: 76, 77, 78, 79, 80, 81, 82

## Blocked

- **Linux Steps 41, 42, 64, 83 and the Linux half of 43**: AGENTS.md §3 forbids Linux work on
  this Windows PC (maintainer decision 2026-11-04); they wait for the Linux server phase.
- **Maintainer manual checks** (implemented and merged; issues stay open until the visible-window /
  real-input / real-clipboard / multi-monitor checks named in each issue's report are done):
  #8, #14, #15, #16, #18, #19, #20, #55–#58, #61–#64, #71, #73–#80.

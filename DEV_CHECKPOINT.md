# Development checkpoint

Sequential workflow ([ADR 0055](docs/adr/0055-single-branch-sequential-development.md)): one
session, one folder, one branch (`main`), one Step at a time. Update this file at every Step
transition. Resume prompt: *"Continue the Kim sequential development mission from the latest
checkpoint."*

## Now

- Branch: `main` · last pushed commit: see `git log -1 origin/main`
- Current Step: **Step 23** (#23) — tmux prefix and key binding engine
- Last fully completed Step: Step 22 (#22, pane splits and layout, ADR 0056)
- Uncommitted changes: none expected between Steps
- Next action: read #23; ship the `prefix` leader and the `@prefix` bindings of the final
  manifest (`docs/contracts/examples/kim-hotkey-manifest-v1/subak.json`) in `subak-gui`
  (kim-hotkeys already parses leaders), the pass-through rule (HOTKEY_CONTRACT §13.1), help UI,
  and the CLI attach prefix from `hotkeys.json`

## Verification log (latest first)

- 2026-11-11: Step 22 — `cargo xtask ci` green (after fixing hotkey scenario 13); pane_layout 5/5,
  gui split test, e2e step22 2/2 (focused local run, LEDGER).
- 2026-11-11: Daily e2e had failed 7× (stale `step17`/`step18` scenarios); fixed, focused local
  run 4/4 PASS; recorded in `e2e/LEDGER.md`.

## Queue (Step number in the issue title; reconstruct with
`gh issue list --repo hellomouv/kim --state open --limit 200`)

1. Subak: 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 43
   (Windows part), 44
2. Noeul: 60, 61, 62, 63
3. Ddalgak: 76, 77, 78, 79, 80, 81, 82

## Blocked

- **Linux Steps 41, 42, 64, 83 and the Linux half of 43**: AGENTS.md §3 forbids Linux work on
  this Windows PC (maintainer decision 2026-11-04); they wait for the Linux server phase.
- **Maintainer manual checks** (implemented and merged; issues stay open until the visible-window /
  real-input / real-clipboard / multi-monitor checks named in each issue's report are done):
  #8, #14, #15, #16, #18, #19, #20, #55–#58, #61–#64, #71, #73–#80.

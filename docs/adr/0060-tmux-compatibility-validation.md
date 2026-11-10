# ADR 0060 — tmux compatibility: validated by the build, stated without overclaiming

- **Status:** Accepted (Step 26, 2026-11-11)
- **Requirements:** `SUBAK-FR-060`, `SUBAK-FR-013`, `SUBAK-FR-018`
- **Extends:** [ADR 0059](0059-subak-advanced-tmux-behaviours.md)

## Context

Step 26 asks for a test-evidenced tmux matrix that reflects working code, core journeys that
pass on Windows, and a public statement that claims parity only where tested. A matrix kept by
hand drifts: before this Step 17 rows were still `planned` for Steps finished long ago and one
evidence path pointed at a moved file.

## Decision

1. **The build checks the matrix.** `cargo xtask docs-check` (so `cargo xtask ci`) validates
   `docs/TMUX_COMPATIBILITY.md` §1–§22 with `xtask/src/tmux.rs`: row shape, Status, Target
   (`N`, `N, M`, `post-26`, or `—` for `unsupported`), evidence for `implemented`/`partial`
   rows that **exists** (each cited `.rs` file, and the `fn`/`const`/… it names), a rationale for
   `deferred`/`unsupported`, no `planned` row for a finished Step (`DONE_THROUGH` = 25), and §27
   counts equal to the rows. Evidence must still exercise the row's behaviour (§26); the check
   makes sure it is not stale.
2. **Every row is classified.** The 17 stale `planned` rows were verified against the code:
   9 became `implemented` or `partial` with their tests, 6 `deferred` (follow-ups in #97),
   1 `unsupported`. No row is `planned`.
3. **One journey suite.** `crates/subak/tests/tmux_journeys.rs` keeps one long-running program
   alive through every multiplexer operation and client event; `e2e/tests/step26_tmux_journey.rs`
   repeats it on the release binaries in the daily run.
4. **The statement** at the top of the matrix distinguishes tmux-like UX, command-level
   compatibility and unsupported features, gives the counts, and says Subak is not a drop-in
   tmux. It is updated whenever the counts change (the check enforces §27).

## Consequences

- A Step that changes tmux behaviour must update the matrix in the same commit or CI fails.
- A visible-window, real-keyboard run is still the maintainer's (GUI manual checks in
  SUBAK_GUI §16.5, §17.3); automated GUI evidence uses the off-screen window.

# ADR 0077 — Ddalgak's test playground: one pure simulator, live tests confined to the playground

Status: Accepted (Step 76) · Requirements: `DDALGAK-FR-040`, `DDALGAK-FR-041`, `DDALGAK-FR-042`,
`DDALGAK-SEC-001` · Related: [DDALGAK_RULE_MODEL §7, §11](../DDALGAK_RULE_MODEL.md),
[DDALGAK_GUI_JOURNEYS J5](../DDALGAK_GUI_JOURNEYS.md), ADR 0040 (simulated pointer), ADR 0049
(editor process)

## Context

Step 76 asks for a place to try saved rules, a profile, a mode or an unsaved draft before they
act on the real system: a trace of the decision, a preview of every step, a step-through, a
reset, and an opt-in live test that never reaches other programs and never opens a program or a
website without a confirmation. The contract (`ddalgak-simulation-v1`, 23 examples) has existed
since Step 65; every action type's preview function since its Step (68–72).

## Decision

1. **One pure function, `ddalgak::simulate::simulate(document, input)`**, produces the
   `ddalgak-simulation-v1` trace. It runs §7.1 selection on the document it is given (stepping
   aside, paused, candidates with the mode's parent chain and `when`, precedence) and builds each
   step's preview with the engine's own preview code (`remap::preview`, `text::preview`,
   `launch::url_preview`, `mouse::compile_step` + `mouse::preview` on a simulated 1920×1080
   desktop). It reads no window, no clipboard and no engine state: the user chooses the context.
   Consent is checked like the engine does, including the launch fingerprint.
2. **Two front ends, same function.** `ddalgak test <keys> [context options] [--draft]` (always
   `simulate`, never asks the running app) and the editor's **Playground** page, which can include
   the unsaved rule being edited in place of its saved version.
3. **Live is confined to the playground.** It needs an explicit tick each session; typing and
   shortcuts are written into the playground's own text field, never sent with `SendInput`;
   the pointer, the mode and the engine stay simulated (`performed: false`); a program or website
   opens only after *Confirm and open* for that one step, through the editor's `Launcher`
   (tests use `FakeLauncher`). The trace reports the launch as `confirmed` and `performed`.
4. **Reset** clears the field, the trace and the simulated pointer. Neither mode writes the rules
   file or calls the running engine.

## Consequences

- The CLI, the GUI and the contract examples cannot drift: a unit test reproduces all 23
  examples (decision, effects, previews, counts) and validates every trace against the schema.
- "Live" deliberately does not inject into other programs: the requirement's "allowlisted test
  surface" is the playground's own field. Real-system checks (does the remap reach the game?) stay
  with the running engine and the maintainer's manual scripts.
- `ddalgak explain --json` (RULE_MODEL §11) is not added here; `ddalgak test` answers the same
  question for a chosen context.

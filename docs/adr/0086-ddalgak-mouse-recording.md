# ADR 0086: Ddalgak records the mouse only during an explicit recording

- Status: accepted
- Date: 2026-11-12
- Issue: #101 (follow-up of Step 78, ADR 0079); requirements `DDALGAK-FR-074`, `DDALGAK-SEC-001`

## Context

Step 78's recorder takes keys typed into the editor's own window only, because recording keys
elsewhere is a keylogger. Recording clicks is useful only in other programs, and a click's
point carries far less personal content than a key.

## Decision

1. *Record mouse* in the macro builder starts a recording with the same banner as *Record keys*;
   *Stop recording* ends it. Only then does a low-level mouse hook exist, on its own thread of
   the editor (`ddalgak::win::mouse_record`); it **observes** (every event passes on unchanged).
2. Kept: physical button presses (left, right, middle, X1, X2) with their screen point, and wheel
   turns. Not kept: input other programs injected, presses on Ddalgak's own windows (so *Stop*
   is not recorded), presses over windows of elevated processes (no rule may act on them) and
   anything on the secure desktop (Windows never delivers it). Moves without a press are not
   kept — the point of each press is. No key is ever recorded by this hook.
3. The editor turns the events into steps for review: `mouse_click` at the point (two presses
   within 500 ms and 4 px: `double_click`), one `mouse_scroll` per burst of notches, `delay` for
   pauses over 0.3 s; at most 32 steps. Nothing is written until the user saves; the rule needs
   `input-injection` consent like any mouse rule.
4. Drags are not represented (a press and a release elsewhere is recorded as a click where the
   press happened): `mouse_click` has no separate down/up in a macro (AHK matrix 4.11).

## Consequences

- The threat model's recorder row covers the hook (SECURITY §10).
- The hook itself is checked on the real desktop by the maintainer (DDALGAK_ACCEPTANCE row 23):
  tests install and remove it without sending input.

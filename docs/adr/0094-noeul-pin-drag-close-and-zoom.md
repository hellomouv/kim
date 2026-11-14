# ADR 0094: New pins drag at once, a double-click closes a pin, zoom shows its percentage

- Status: accepted
- Date: 2026-11-15
- Issue: maintainer request 2026-11-15 (§5 zoom percentage, §6 delayed pin dragging, §7
  double-click to close); changes NOEUL-FR-026 and the guard of UX spec §4.2 rule 10 (ADR 0032)

## Context

1. **Dragging a new pin was dead for half a second.** A pin made by the double-click inside a
   selection was given a guard of one double-click time (500 ms by default): every press on it
   was swallowed and `WM_MOUSEACTIVATE` answered `MA_NOACTIVATEANDEAT`, so the rest of a triple
   or quadruple click could not move, zoom or close it. It also ate the user's first drag.
2. **A double-click on a pin did nothing** (NOEUL-FR-026, so that the creating gesture could
   never destroy the new pin). The maintainer wants a double-click to close the pin.
3. **Zoom had no feedback**: the wheel changed a pin's size without saying how much.

## Decision

1. **The guard swallows the gesture, not the user.** For one double-click time after a pin is
   made by a pointer gesture, its release, the middle and right buttons, the menu and a
   double-click (the 3rd and 4th press of a quick series) never act on it, and it is not
   activated — but a left press is no longer eaten: `WM_MOUSEACTIVATE` answers `MA_NOACTIVATE`
   and the press starts the system move loop, so the pin follows the pointer at once. A
   double-click during the guard is handled as a press. No sleep, no timer, no extra click.
2. **A double-click closes that pin** — only that one — when `[pins] double_click = "close"`
   (the default); `"none"` restores the old behaviour. `Shift`+double-click still toggles a
   thumbnail; `Esc` and `Ctrl+W` (the `pin.close` action, rebindable), the context menu and
   `noeul pin close` still close a pin. A double-click *inside a capture selection* still makes
   a pin: the overlay and the pin are different windows and states, and the guard of 1. keeps the
   two gestures apart.
3. **The zoom read-out.** After a pin's zoom changes (wheel, keys, border drag) its window shows
   the percentage (`125%`, `75%`) in a small near-black pill in the picture's top-right corner
   (Segoe UI 12 px at the monitor's scale, `bg.base` at 82 % and `text.primary` of the Kim design
   system) for `duration.hint` (1.2 s), then the surface without it comes back; never at 100 %,
   never on a pin too small to hold it, never on the first present. It is painted only into the
   layered window's surface; copies, saves and exports use the pin's frame and never contain it.
   The window's own one-shot timer removes it (no idle timer). Zoom stays anchored at the
   pointer as before (`zoom::around`).
4. NOEUL-FR-026 now reads: a double-click on a pin closes it by default and can be turned off;
   the clicks left from the gesture that created a pin never close it.

## Consequences

- A pin can be dragged the moment it appears; measured on the real desktop (sandbox instance,
  real mouse input): with the press 0, 0, 50, 100 and 200 ms after the pin became visible the pin
  followed every time, its first movement 62–76 ms after the press (this includes the test
  driver's own 30 ms settle), the full 200 px each time — well within the 300 ms target. Before,
  any press in the first 500 ms was swallowed.
- Triple and quadruple clicks inside a selection make one pin that stays (real desktop).
- Tested: the pin window test (during the guard a press is not eaten and does not activate,
  the release, menu, middle click and a double-click are ignored; afterwards a double-click
  reports `DoubleClick`), the core test (a double-click closes only that pin; `none` does
  nothing), the read-out test (painted only inside its pill in the top-right corner, not at
  100 %, not on a tiny pin) and the window test (shown after a change, removed by its timer,
  never at 100 %), the IPC and CLI schemas (`pins.double_click`), the e2e scenario
  `adr0094_noeul_pin_gestures_and_settings`, and the real desktop (`125%` appeared after two
  wheel notches and was gone 1.6 s later; a double-click closed only the pin under it).

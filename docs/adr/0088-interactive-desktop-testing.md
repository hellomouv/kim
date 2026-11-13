# ADR 0088: Real interactive desktop testing on the development PC

- Status: accepted
- Date: 2026-11-13
- Issue: maintainer decision 2026-11-13 (affects every open maintainer-check issue, e.g. #56,
  #57, #61–#64, #66, #68, #87); replaces the "work headlessly" rule of AGENTS.md §5

## Context

Until now agents worked headlessly on this PC because the maintainer uses it at the same time:
no visible windows, no synthesized input, no real clipboard, no settings changes. Every check
that needs the visible desktop (real `F1`, real mouse gestures, clipboard round trips, 150 %
scaling, double-click speed) was left to the maintainer, and the issues stayed open.

## Decision

1. The agent may drive the **real interactive desktop**: move and click the mouse, send keys,
   open, resize and close windows, launch Kim, Subak, Noeul and Ddalgak GUIs, register real
   global hotkeys, capture the screen and use the real clipboard.
2. It may **temporarily** change the keyboard, mouse, clipboard, display-scaling and
   double-click settings a check needs. Before a change it saves the prior state; after the
   check it restores it (the clipboard's text and image formats it read, the double-click time,
   the scaling) and verifies the restore. Nothing is changed permanently.
3. Windows and programs the maintainer already has open are not touched; tests use their own
   instances (own Notepad, sandboxed tool profiles, test-created Subak sessions).
4. Nothing bypasses Windows security: no UAC elevation, credential prompt or lock-screen
   automation. With the session locked, interactive checks wait; non-interactive work goes on.
5. While a session runs, a **session-scoped keep-awake helper** holds
   `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` (the display may still turn
   off; no power-plan change) and, every five minutes on an unlocked desktop with no test
   holding the pointer, nudges the cursor 2 px and back without clicking. It exits with the
   Claude Code process and leaves no scheduled task or service. It is a development aid outside
   the product, not part of any Kim tool.
6. A check that the hardware cannot do (a second physical monitor, HDR) is recorded as **not
   tested** with the limitation; the requirement stays open.

## Consequences

- Maintainer-check acceptance scripts can now be run and recorded with real evidence, and
  failures they find are fixed like any defect.
- The product rules are unchanged: tools' headless/CLI paths still never open windows, and
  automated tests still run off-screen; the real-desktop runs are an extra, manual layer.

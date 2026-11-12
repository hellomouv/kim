# ADR 0084: Ddalgak one-shot timers, catching up after sleep, and notifications without WinRT toasts

- Status: accepted
- Date: 2026-11-12
- Issue: #102 (follow-up of Step 79, #84, ADR 0080); requirements `DDALGAK-FR-076`, `DDALGAK-NFR-001`

## Context

Step 79 gave schedules (every N minutes, at a time on chosen days) that skip times missed while
the computer slept, and a `notify` step that is a balloon of Ddalgak's tray icon. #102 asked for
AutoHotkey's one-shot timers (`SetTimer` with a negative period), a choice of behaviour after
waking, and Windows toasts "when the tray icon is hidden".

## Decision

1. **`timer` step** `{ "rule", "after_ms" }`: arms a one-shot timer that starts another rule of the
   same profile once, `after_ms` later (1 ms–24 h), while the engine runs. Like AutoHotkey,
   arming the same timer again restarts its countdown (one timer per target rule). The worker
   keeps the deadlines in its one wait (no new thread, no periodic timer: `DDALGAK-NFR-001`). A
   pause, a turn-off, a reload, a profile switch and the emergency cancel every timer. A timer
   for its own rule is refused by the validator and the loader (it would be a hidden loop; a
   periodic rule is a schedule). The step needs no permission; the started rule runs with its
   own consent, conditions and outcome.
2. **`schedule.missed`**: `skip` (default, as before) or `run_once`: a time missed while the
   computer slept runs once when the worker wakes (several missed times run once). Times passed
   while paused or off still never run: the user turned Ddalgak off.
3. **Toasts.** The real app always has a tray icon (only the debug test host has none). The
   Windows shell shows a `NIF_INFO` balloon as a toast notification on Windows 10 and later,
   also when the icon is in the overflow area, and keeps it in the notification center. A
   WinRT `ToastNotification` would add nothing visible but needs an AppUserModelID registered on
   a Start-menu shortcut, which Kim's installer would have to create and remove for Ddalgak.
   That cost is not justified now; the balloon path stays. Revisit if the shell stops converting
   balloons or a "no tray icon" option is added.

## Consequences

- Rules v1 gains the `timer` step type and `schedule.missed` (additive; older versions report
  `step_unsupported` or `unknown field`).
- The real-desktop check of a toast with the icon in the overflow area and of `run_once` after a
  real sleep stays a maintainer check (DDALGAK_ENGINE §20).

# ADR 0080 — Ddalgak window, clipboard, notification and tool steps, and schedules

Status: Accepted (Step 79) · Requirements: `DDALGAK-FR-072`, `DDALGAK-FR-073`, `DDALGAK-FR-076`,
`DDALGAK-SEC-008`, `DDALGAK-SEC-011`, `KIM-SEC-001` · Related:
[DDALGAK_RULE_MODEL §3.5, §5.14–§5.17](../DDALGAK_RULE_MODEL.md), ADR 0079 (macros),
[INTEROP](../INTEROP.md)

## Context

Step 79 asks for actions beyond key handling — windows, the clipboard, notifications, scheduled
execution and the output of other Kim tools — usable from the GUI, macros and the playground,
with permissions, focus, power and privileged-window limits handled transparently, and with
other Kim tools optional and never installed by Ddalgak.

## Decision

1. **Four additive step types** in `ddalgak-rules-v1`: `window` (activate, minimize, maximize,
   restore, move/resize, always on top; the active window or a program's visible window),
   `clipboard` (set, clear, transform: plain, trim, upper, lower), `notify` (a tray balloon) and
   `tool` (a capability through `kim tools invoke`). They run as macro stages (ADR 0079), alone
   or mixed. An older Ddalgak keeps them as future steps (inactive).
2. **Permissions**: a new `window-control` consent for `window`; `clipboard` steps need the
   existing `clipboard` consent. `notify` and `tool` need none from Ddalgak: Kim applies the
   tool's own consent, and Ddalgak passes no consent flags, so a capability that needs the user's
   consent is refused by Kim (`consent_missing`).
3. **Privileged windows and focus**: window steps never touch a window of a process with a higher
   integrity level (`skipped_elevated_target`) or anything while the session is locked
   (`skipped_secure_desktop`). They use asynchronous calls (`ShowWindowAsync`,
   `SWP_ASYNCWINDOWPOS`), so a hung program never stalls the worker. Bringing a window to the
   front may be refused by Windows (`focus_refused`). Windows are found by process image, never by
   title.
4. **Clipboard privacy**: the content is never logged, recorded or previewed (counts only); texts
   put on it are excluded from clipboard history and cloud sync; a clipboard held by another
   program is `clipboard_busy`.
5. **Schedules**: `trigger.schedule` — every N minutes (1–1440) from when the engine starts, or at
   a local HH:MM on chosen days. The worker keeps one wall-clock deadline per scheduled rule and
   sleeps until the nearest. Schedules fire only while the engine runs (`Msg::Engine`). A time
   missed by more than a minute (the computer slept) is recorded once as `skipped_missed` and
   never caught up; times passed while paused or off are skipped silently. A scheduled rule never
   injects input (the schema and the loader refuse it). No schedule means no timer.
6. **Tools**: Kim is found through its install record (`%LOCALAPPDATA%\Programs\mouv\kim\
   current.json`; `KIM_EXE` in tests). It runs hidden for at most 30 s. A missing Kim or provider
   is `tool_missing`; Ddalgak never installs anything.
7. **Test host**: window and notification steps are recorded in the test folder
   (`desktop.jsonl`), never carried out; the real clipboard is never touched.

## Consequences

- Every new effect reuses the macro runner, its cancellation and its one activity entry.
- New activity outcome codes are part of `ddalgak-cli-v1` (and the launch codes that were missing
  there are added too).
- Not in this Step (#102): `run_command` with per-command trust, a window picker and
  autocomplete in the editor, a `tool` form, toasts without a tray icon, one-shot timers.

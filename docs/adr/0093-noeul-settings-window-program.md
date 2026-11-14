# ADR 0093: Noeul's settings window is a program of its own, in the Kim design system

- Status: accepted
- Date: 2026-11-15
- Issue: maintainer request 2026-11-15 (§8 "Noeul — Completely Improve the Settings UI");
  supersedes the window part of Step 61 / ADR 0075; keeps ADR 0023 (Noeul links no GUI
  framework, a 20 MB idle budget) and ADR 0092 (one design system)

## Context

Step 61 gave Noeul a settings window of standard Win32 controls: one list box of every action,
push buttons, two combo boxes and three check boxes on one page. It worked with the keyboard
and screen readers, but it looked nothing like Kim, covered only a part of `config.toml`, and
the maintainer found it unattractive and hard to understand. Rebuilding the same controls in
owner-drawn GDI would duplicate the egui components of `kim-ui`; linking egui into
`noeul.exe` would break ADR 0023 for a window most users open rarely.

## Decision

1. **`noeul-settings.exe`**, a second program in the Noeul package (as `ddalgak-editor.exe` is
   for Ddalgak, ADR 0049), built with `kim-ui`. Noeul starts it when the user opens Settings
   (tray menu, `noeul settings`, Kim's *Settings…*), passing its `--endpoint` when it is not the
   default one; one window per user session (a second start brings it forward). The resident
   `noeul.exe` stays free of GUI frameworks. When the program is missing (or under the
   debug-build test hook) the Step 61 window still opens, so nothing is lost.
2. **Every change is the documented command.** The window runs `noeul.exe` from its own folder
   (`config set`, `hotkeys set|disable|reset|import|export`, `pin group …`) on a worker thread
   and reads `config show`, `hotkeys list` and `pin group list` again after each answer; the
   settings model is Noeul's own (`noeul::prefs::Model`). `config set` makes the running
   instance read the file again, as before.
3. **Sections** on the left rail: *General* (tray icon and its clicks), *Capture* (detection,
   magnifier, pointer, double-click and middle-click inside a selection, keyboard help, quick
   save folder), *Pins* (double-click a pin, backup, groups of the running instance),
   *Shortcuts* (every action: click its field and press keys, or *Pick* keys such as
   `PrintScreen` that cannot be pressed in a window; Off, Reset, Reset all, Import, Export),
   *Appearance* (this window's theme, the language of Noeul's menus and descriptions, pin
   outline and shadow) and *Advanced* (history, privacy, files and clipboard, colour picker).
   Switches for on/off values, segmented choices for up to three values, dropdowns beyond;
   numbers and the folder are checked inline before anything is written. All settings of
   `config.toml` that a user changes are reachable; nothing that existed was removed.
4. **Recording keys**: Windows delivers a registered global hotkey to its owner, not to the
   focused window, so Noeul's own keys (`F1`, `F3`) could never reach a separate recorder. The
   window asks the running instance over its verified IPC pipe (`hotkeys.pause`, a new method of
   IPC v1) to release its global hotkeys for at most 30 s while it records, renewing the pause
   while recording lasts and ending it (`seconds: 0`) when a key is taken, recording is
   cancelled or the window loses focus. The instance arms a one-shot timer so the hotkeys come
   back by themselves after at most 120 s, even if the window crashed.
5. This window's theme is saved in `settings-window.json` beside `config.toml`; the language
   follows `[general] language` (Noeul's menus use the same setting).

## Consequences

- Noeul's settings look and behave like Kim's and the Ddalgak editor's; `noeul.exe`'s idle
  memory is unchanged (the window's process exists only while it is open).
- The package grows by one program; `xtask package noeul` ships it.
- Tested: `noeul-settings` UI tests (a fake `noeul` answers and records every command: every
  section on the rail, a switch writes `config set general.tray_icon false`, a segmented choice
  writes `capture.detection element`, `pins.double_click none`, recording pauses the hotkeys and
  binds `F2` then resumes them, *Pick* binds `PrintScreen`, Korean descriptions, a number out of
  range is refused inline), Noeul's IPC schema test (`hotkeys.pause` both ways), its unit test
  (pause, renew, resume, the 120 s cap) and the real desktop (sandbox instance on a private
  endpoint: the window opened from `noeul settings`, dark and light, a real click on *Nothing*
  wrote `double_click = "none"` and *Close* wrote it back).

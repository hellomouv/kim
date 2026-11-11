# ADR 0075 — Noeul's settings window

- **Status:** Accepted (Step 61, 2026-11-12)
- **Requirements:** `NOEUL-FR-100`, `NOEUL-FR-101` (in part), `NOEUL-FR-103` (in part),
  `NOEUL-FR-111`, `NOEUL-NFR-012`, `NOEUL-NFR-013`, `NOEUL-SEC-012`, `KIM-FR-013`
- **Builds on:** [ADR 0017](0017-universal-customizable-hotkeys.md) (the hotkey contract and its
  CLI), [ADR 0032](0032-noeul-double-click-pin-and-outputs.md) (gestures)

## Decision

1. **One compact window of standard Win32 controls** — a list box of every action with its keys,
   push buttons, combo boxes, check boxes, a read-only recorder field and a status line — opened
   only by `noeul settings` and the tray's "Settings…" (a headless command never opens it,
   `NOEUL-FR-111`). Standard controls bring keyboard navigation (`Tab`, arrows, `Alt`+mnemonics
   through `IsDialogMessage`), screen-reader names (each control's text), High Contrast (system
   colours) and the user's text size for free; the system message font at the monitor's DPI and
   `WM_DPICHANGED`; a dark title bar and dark controls when Windows apps use the dark mode.
2. **Every change is a `noeul` command.** Change/Disable/Reset/Reset all/Import run `noeul hotkeys
   set|disable|reset|import`, the gestures and switches `noeul config set …` — the same checks
   (conflicts, reserved keys, validation, atomic writes with a backup) and the same files as the
   CLI; the window reads its rows back from `noeul hotkeys list` and `config show`. The commands run
   on a worker thread because they may call this instance over IPC (`hotkeys.reload`); the window
   is "busy" until the answer and the reload arrive.
3. **Recording never triggers.** While the recorder records, the instance ignores its global
   hotkeys (`Shell::settings_recording`) and the recorder takes every key (`DLGC_WANTALLKEYS`),
   so `F1` or `F3` can be recorded without capturing or pasting; `Esc` alone cancels; modifiers
   alone wait.
4. **Conflicts in words.** A binding over a default is allowed and the status says what it
   shadows (the CLI's own message); a conflict with another user binding is refused with the CLI's
   message and "Use anyway" retries with the flag its hint names (`--replace` takes the keys from
   the other action, `--allow-conflict` accepts a cross-tool global conflict).
5. **No modal dialog except the file pickers.** "Reset all…" asks in the status line and resets on
   a second press (accessible, and testable without a dialog).
6. **Double-click pins by default**, shown as "Pin it (Noeul default)"; the keyboard alternative
   (`Enter`, `capture.pin`) is listed with the other keys.

## Consequences

- The window has no timer and exists only while open; nothing runs in the background for it.
- A change made in the window is exactly what the CLI would do; tests drive the real window with
  its own messages under the debug-build test hook (off-screen, never activated).

## Not done

The other sections of `NOEUL-FR-101` (start at sign-in, language, capture options, privacy,
About), per-section reset, the Kim colour tokens inside the window, Korean text. **Not tested:**
the window on the maintainer's screen (focus, a real keyboard, Narrator, a 150 % monitor,
dark/light switching while open).

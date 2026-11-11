# Ddalgak user guide

Status: **current** (Step 82) · Ddalgak (딸깍, "click") turns keys you choose into actions: remap
keys, type text, open programs and websites, move and click the mouse, switch modes, run macros,
arrange windows, use the clipboard and show reminders. It is part of Kim and installed only when
you ask (`kim install ddalgak` or Kim Manager).

## First run

Start **Ddalgak** from the Start Menu. Its tray icon appears; the engine is **off** and there are
no rules. Nothing starts at sign-in unless you turn it on (tray menu or `ddalgak startup
enable`), and nothing updates itself (`kim update ddalgak` when you want).

**Emergency:** `Ctrl+Alt+Shift+Esc` always turns the engine off and releases every key it holds.
It cannot be changed or taken by a rule.

## Making rules

Open the editor (tray menu, or `Ctrl+Alt+Shift+F15`): **+ New rule**, press the keys, choose what
happens (a tile), fill in the details, **Try** it, **Save and enable**. Rules that type, launch
programs or control windows ask for your permission first, naming exactly what they do.

- **Several steps**: build a macro step by step, or **Record keys** in the editor (only what you type
  into the editor is recorded; you review every step before saving).
- **Modes** (Modes page): rules that apply only in a mode — for example arrows on H/J/K/L in a
  `nav` mode — switched by a rule, the tray or `ddalgak mode set nav`.
- **Programs**: *Only in apps* limits a rule to programs (Code.exe), *Not in apps* excludes them,
  and a window title can be required. Windows Ddalgak cannot see (administrator windows, the
  sign-in screen) never match.
- **Schedules**: *On a schedule instead of keys* runs reminders, window or clipboard steps every N
  minutes or at a time, only while the engine is on; missed times are skipped.
- **Profiles** (Profiles page): complete sets of rules for work, games or development; switch with
  a side-by-side diff, keep versions, roll back, export and import (including AutoHotkey v2
  scripts — [DDALGAK_AHK_MIGRATION](DDALGAK_AHK_MIGRATION.md)).

## Trying safely

The **Playground** page shows what a key would do in a profile, mode and program — which rule wins,
every other candidate and why, and each step — without typing or opening anything. *Live* types
only into the page's own field and opens a program or website only after you confirm it.

## Privacy

Ddalgak keeps no keystroke history and sends nothing anywhere. The activity list records rule IDs
and outcomes only; typed text, clipboard contents, window titles and website paths are never
logged. Hotstrings keep only the last few characters in memory, never in password fields.

## Scripts and agents

Everything also works from the command line with `--json` ([DDALGAK_MCP](DDALGAK_MCP.md)):
`ddalgak rule list`, `ddalgak test <keys>`, `ddalgak profile apply work`, `ddalgak pause`, …
An optional MCP adapter lets AI agents read and explain your rules; changes need your explicit
grant, and agents can never run macros, type or grant permissions.

## Limits

Windows 10 and 11 only (Linux later). Ddalgak is not AutoHotkey and does not run `.ahk` scripts
([DDALGAK_AHK_MATRIX](DDALGAK_AHK_MATRIX.md) compares them feature by feature). Keys typed into
administrator windows or on the secure desktop are not seen or changed; some games with their own
input handling may ignore synthetic input.

# Ddalgak — real Windows acceptance script

Status: **current** (Step 82) · Run by the maintainer on a real Windows 10/11 desktop with a real
keyboard and mouse, release build installed through Kim (or `cargo xtask package ddalgak`). Each
row records **pass / fail / not run** with the date and machine; a failing row is filed as an
issue and blocks the release ([RELEASE_READINESS](RELEASE_READINESS.md)). The automated evidence
for each row is listed so the manual run only checks what tests cannot: real devices, real
programs, the visible windows and the screen reader.

| # | Check | How | Automated evidence |
| --- | --- | --- | --- |
| 1 | First run is safe | Install with Kim, start Ddalgak from the Start Menu: tray icon, engine **off**, no rules, the editor's empty state with three starters; nothing starts at sign-in (`ddalgak startup status`: disabled); no network traffic | flows `j1_first_start_is_safe…`, e2e step82 |
| 2 | Remap | Rule *CapsLock acts as Ctrl, tap = Escape*; turn the engine on; in Notepad `CapsLock+A` selects all, a quick tap is Escape | core remap tests, hook_windows `real_remap_block_and_dual_role…` |
| 3 | Unicode text | Rule `Ctrl+Alt+Shift+F21` types `감사합니다 👍` (and a Hangul IME on): Notepad shows exactly that; nothing in a password field | run tests, worker typing tests |
| 4 | Open a program | Rule opens Notepad (`%SystemRoot%\System32\notepad.exe`); consent dialog names it; it opens once; `bring to front` activates an open one | launch_windows tests, e2e step70 |
| 5 | Open a website | Rule opens `https://github.com/` in the default browser once per press | e2e step70 |
| 6 | Pointer | Rule moves the pointer 200 px right and clicks; a grid move; a held button (drag) is released when the key goes up | mouse tests, hook_windows mouse test |
| 7 | Mode toggle | Rule toggles mode `nav` (H/J/K/L → arrows only in `nav`); the tray shows the mode | modes tests, e2e step72 |
| 8 | Enable/disable | `ddalgak pause`/`resume`, the tray's Pause, a rule's *Turn off*, a profile disabled — each stops exactly what it says | e2e step73 |
| 9 | GUI CRUD | In the editor: create, edit, duplicate, reorder (drag and Alt+Up), delete with Undo, search and filter; Korean labels; keyboard only; Narrator reads captured keys | flows j2–j7 |
| 10 | Profiles | Save a version, switch profiles with the diff (a held CapsLock is released on the switch), roll back, export, import (also an `.ahk` file) | e2e step75, step80 |
| 11 | Shortcuts | Ddalgak's own shortcuts (`Ctrl+Alt+Shift+F13`–`F15` by default) work and can be changed in the hotkeys file; a conflict with another Kim tool is reported | shortcuts tests, step74 e2e |
| 12 | Test playground | Simulate a key in a chosen profile, mode and program — nothing is typed; Live types only into the page's field; a website opens only after *Confirm and open* | playground flows, e2e step76 |
| 13 | Macro chain | Macro: open Notepad, wait 1 s, type `hello`, Enter, type `world`; it runs in order; the emergency chord during the wait stops it with no key held | worker macro tests, e2e step78 |
| 14 | Emergency disable | With remaps active and a key held, `Ctrl+Alt+Shift+Esc` turns the engine off and releases every held key; it cannot be remapped | safety tests, e2e step73 |
| 15 | Crash recovery | Kill `ddalgak-gui.exe` in Task Manager while CapsLock (remapped) is held: no key stays down; the next start says *unclean exit* and keeps the engine off | recovery tests, e2e step73/75 |
| 16 | Elevated and secure | An elevated Notepad: no typing, no window step (`skipped_elevated_target`); UAC prompt and lock screen: nothing fires | context tests, e2e step77 |
| 17 | Idle and memory | 100 rules, engine on, 10 minutes idle: Task Manager shows 0 % CPU and **< 25 MB** for `ddalgak-gui.exe` (release); editor open < 80 MB | `cargo xtask bench-ddalgak` (debug upper bounds) |
| 18 | High DPI | 150 % and 200 % scaling, two monitors with different scaling: the editor, the tray menu and pointer moves are correct | mouse DPI tests |
| 19 | Long run | Leave the engine on for a day of normal use: no handle growth (Task Manager handles column), no stuck key, no missed emergency | — (manual) |
| 20 | Updates and removal | `kim update ddalgak` only when asked; `kim uninstall ddalgak` keeps the rules unless `--purge-data` | Kim e2e step40, step81 |
| 21 | Commands, timers, notifications (#102) | A rule `Ctrl+Alt+Shift+F21` → `run_command` of a small script: the consent dialog names the program and every argument; it runs with no window; a non-zero exit shows `command_failed`. A `timer` of 10 s for a `notify` rule shows one toast 10 s later. With the tray icon in the overflow area a `notify` still shows a toast. A schedule with *run it once when it wakes*: sleep five minutes, it runs once | `win_commands_…`, `a_timer_starts_…`, e2e issue102 |
| 22 | Pickers (#102) | *Running programs ▾* lists the open programs (no titles) and fills the field; with Kim and Noeul installed *Find installed capabilities* lists `pin.list`, `image.pin`, `screen.capture-file`; both work with the keyboard and Narrator | `running_programs_…`, e2e step81 |

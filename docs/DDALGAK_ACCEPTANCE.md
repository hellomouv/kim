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
| 11 | Shortcuts | Ddalgak's own shortcuts (no defaults ship, HOTKEY_CONTRACT §7.5; bind e.g. `Ctrl+Alt+Shift+F13`/`F14` in `%APPDATA%\mouv\ddalgak\hotkeys.json`) work; a conflict with another Kim tool is reported | shortcuts tests, step74 e2e |
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
| 23 | Macro blocks and mouse recording (#101) | Build a macro with *Repeat block* and *If block* (in Notepad / otherwise) with the keyboard; in Notepad and elsewhere it takes the right branch. *Record mouse*: click twice in Notepad and turn the wheel, then *Stop recording*: the list shows a double click at the point, a delay and a scroll; clicks on the editor and on an elevated window were not kept. `ddalgak rule run <id>` runs it without its keys | `blocks_unroll_…`, `variables_and_branches_…`, `mouse_events_become_steps`, e2e issue101 |
| 24 | Conditions and profiles by program (#100) | A rule with *Window class* `Chrome_WidgetWin_1` works in Chrome or VS Code only; a rule *only 09:00–17:00 on weekdays* is quiet on Saturday. *Switch profiles by program* with `notepad.exe` → a second profile: focusing Notepad switches (tray tooltip says so), another program switches back, an elevated Notepad never switches; turning it off returns to the saved profile; rules.json still names the saved profile | `window_class_and_time_conditions`, `app_profiles_choose_…`, e2e issue100 |

## Results

### 2026-11-13 — Windows 10 Pro 19045, one monitor at 100 %, release build from `main`

Run by the agent on the real desktop (ADR 0088) with real windows, real `SendInput` keys and
mouse clicks, in a sandboxed profile (`APPDATA`/`LOCALAPPDATA` redirected to a scratch folder,
test-started Notepad only; the maintainer's programs were not touched). Injected keys are
ignored by rules by design (`skipped_injected_input`), so a rule's *action* is checked through
`ddalgak rule run <id>` or a rule with `injected: accept`; what needs a physical key press is
**not run** below.

| # | Result | Evidence |
| --- | --- | --- |
| 1 | **pass** (without the Kim install) | fresh profile: engine off, no rules, `startup status` disabled; the editor's empty state |
| 2 | **not run** | needs a physical CapsLock press (injected keys never trigger a remap) |
| 3 | **pass** after a fix | `rule run` typed exactly `감사합니다 👍` into Notepad; a WinForms password box first received the text — fixed in `1781540` (any `*EDIT*` class with `ES_PASSWORD`), then `skipped_password_field` with 0 characters |
| 4 | **pass** | F22 (`injected: accept`) opened Notepad once (1 → 2 processes); paused and turned-off rules opened nothing |
| 5 | **not run** | would open a tab in the maintainer's running browser |
| 6 | **pass** after a fix (held button not run) | `rule run`: move 2000,600 → 2200,600; a click at 2000,520 moved Notepad's caret 0 → 70; a double click selected the word "5"; 3 notches scrolled the first visible line 4 → 16. A held button needs a physical key (an injected-accepting hold is refused by the recursion guard). The editor's screen map stretched a 5120 × 1440 monitor into a near square — fixed in `91a13d8` |
| 7 | **pass** after a fix | F18 toggled `nav` and back; tray tooltip `Ddalgak: engine on · mode nav`, notched disc; a *hold* rule kept `nav` only while the chord was held (0 keys down after); `mode set` from the CLI. The editor's header kept showing the start-up mode — fixed in `82ae844` (refreshed every second) |
| 8 | **pass** | pause → `skipped_paused`; rule off → nothing; rule on again → opens |
| 9 | **pass** after fixes (Narrator not run) | editor with the mouse and keys: create (key capture, *Type text*, Korean text, simulation, consent with Cancel focused) → `rule run` typed `안녕하세요 GUI`; rename; turn off/on (detail pane and row checkbox); duplicate (Ctrl+D, named "… (사본)", off); reorder by Alt+Up/Alt+Down and by dragging the handle; delete with confirmation (mouse and Delete/Tab/Enter) and Undo; Korean search; draft discard asks first. Fixed in `2b8804d`: English field problems, rule names and status lines in the Korean UI, 놀이터 → 연습장, "on 1 lines", a stale Undo toast that undid a later change |
| 10 | **pass** after a fix (held-key release on switch not run; `.ahk` import not run here, flows test) | new profile, switch with the per-key diff (engine follows), duplicate, export to a chosen path (99 rules, all off), delete, import (inactive, 0 of 99 on, engine unchanged), history: side-by-side compare and restore of r105. Fixed in `2b8804d`: the profile-per-program rows did not follow added/deleted profiles |
| 11 | **pass** (shortcuts); conflict report **not run** | `engine.toggle` on `Ctrl+Alt+Shift+F13` and `engine.pause` on `F14` registered; each press switched off/on and paused/resumed with origin `shortcut:engine.*`. The cross-tool conflict report needs installed tools (sandbox has none) |
| 12 | **pass** | simulate: `typ 실행됨 (시뮬레이션)`, nothing typed; Live (after the acknowledgement) typed only into the page's field, Notepad unchanged; the program rule opened nothing until *확인하고 열기*, then exactly one Notepad (closed again); reset kept the engine and its bindings |
| 13 | **pass** (started by `rule run`; the key itself needs a physical press) | hello, Enter, 1 s → 2 s wait, world: `hello⏎world` in order; `ddalgak pause` during the wait: only `hello⏎`, outcome `cancelled`, 0 keys held. A macro built in the editor (typed step, recorded keys, a step removed, repeat 2) typed exactly `abcxyabcxy` |
| 14 | **not run** | the emergency chord is read from physical key state; injected chord correctly ignored |
| 15 | **pass** (no key held) | killed `ddalgak-gui.exe` with the engine on: 0 keys down; next start `off_reason: unclean_exit`, engine off |
| 16 | **not run** | starting an elevated Notepad needs a UAC prompt, which the agent does not trigger |
| 17 | **pass** after a fix (60 s idle, not the full 10 minutes) | 99–100 rules, engine on: idle CPU 0 ms over 60 s; Task Manager memory was 26.9 MB (each sub-schema validator kept a copy of the schema's `$defs`) — fixed in `1781540`: **3.4 MB** (private bytes 4.4 MB), 4 MB after the editor session; editor **55–69 MB** (< 80 MB) |
| 18 | not run in this pass | one monitor only (second monitor: hardware missing) |
| 19 | not run in this pass | |
| 20 | not run in this pass | needs the Kim install path |
| 21 | **pass** after a fix (commands and sleep not run) | a `notify` rule showed its toast from `ddalgak-gui.exe`; a 3 s `timer` started its notify rule 3 s later with the toast; a timer aimed at a mode rule was accepted and did nothing — refused since `77f0271`; an `every_minutes: 1` schedule fired while running and did not run for a time that passed while paused |
| 22 | not run in this pass | |
| 23 | **pass** (keys; mouse recording needs physical input) | *Record keys*: red banner with an explicit stop, the review list with ↑ ↓ edit ✕ before saving; *Record mouse* keeps physical input only (injected clicks: 0 kept, by design) |
| 24 | **pass** (conditions; profile-by-program switching not run) | one chord, two rules: in Notepad `ctx-np` (app condition) won, elsewhere `ctx-any`; `explain` and the Playground show winner, `lower_precedence` and `condition_false` |


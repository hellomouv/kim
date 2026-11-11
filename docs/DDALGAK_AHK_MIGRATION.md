# Moving from AutoHotkey to Ddalgak

Status: **current** (Step 80, [ADR 0081](adr/0081-ddalgak-autohotkey-import.md)) · Requirement:
`DDALGAK-FR-063` · Related: [DDALGAK_AHK_MATRIX](DDALGAK_AHK_MATRIX.md) (feature by feature),
[DDALGAK_RULE_MODEL](DDALGAK_RULE_MODEL.md)

Ddalgak is **not** an AutoHotkey runtime and does not run `.ahk` scripts. It imports the
*declarative* part of an AutoHotkey v2 script — hotkeys, remaps, hotstrings and a few simple
commands — as Ddalgak rules you can see, test and turn on one by one. Everything else is reported,
line by line, as needing manual migration. Nothing is approximated silently, and nothing in the
script is ever executed or evaluated.

## 1. Importing a script

```text
ddalgak profile import my-hotkeys.ahk --dry-run          # the report and the preview; writes nothing
ddalgak profile import my-hotkeys.ahk                    # adds one profile, every rule off
ddalgak profile import my-hotkeys.ahk --dry-run --json   # the same for scripts and agents
```

In the editor: **Profiles → Import profile…**, choose the `.ahk` file, **Check the file**. The
dialog shows how many rules were made, every line that needs manual migration (with its line and
column), then the usual import preview: each rule's trigger and steps, the permissions it will ask
for, the exact programs and websites, conflicts. **Add them, not active** adds a new profile:

- it is never switched to;
- every rule is **off** and has **no consent** — turn each on after reviewing it (Ddalgak asks for
  permission to type, launch or control windows then);
- the script is UTF-8 text (AutoHotkey v2's default); it is only read.

A script in which nothing converts is refused; its `--dry-run` report still lists every line.

## 2. What converts

The repository's fixtures are the reference:
[01-convertible.ahk](contracts/examples/ddalgak-ahk-v1/01-convertible.ahk) and its expected
[document and report](contracts/examples/ddalgak-ahk-v1/01-convertible.expected.json);
[02-manual.ahk](contracts/examples/ddalgak-ahk-v1/02-manual.ahk) lists constructs that do not.

| AutoHotkey v2 | Ddalgak rule |
| --- | --- |
| `CapsLock::Ctrl`, `RAlt::RCtrl` (a key remapped to a key) | `remap` (`Ctrl` → `LCtrl`, `Alt` → `LAlt`, `Shift` → `LShift`) |
| `Insert::return`, `F1::{}` | `block` |
| `^!j::Send "{Down}"`, `^+v::Send "^c{Tab}^v"` | `send_keys` (`Ctrl+Alt+J` → `Down`; `Ctrl+C Tab Ctrl+V`) |
| `^!s::SendText "Kind regards,``nKim"`, `Send "text"` | `type_text` |
| `~F14::…` | `pass_through: true` |
| `^F15 up::…` | `on: release` |
| `*F13::…` | the exact keys (a warning: `*` matched any modifiers) |
| `^!g::Run "https://github.com/"` | `open_url` |
| `^!n::Run "notepad.exe"` (Notepad, Calculator, Paint, Explorer, Character Map) or `Run "C:\…\tool.exe arg"` | `open_program` (`%SystemRoot%\System32\…`, or the full path; arguments split at spaces with a warning) |
| a hotkey block of `Send`/`SendText`/`Run`/`Sleep`/`TrayTip`/`Win…`/`A_Clipboard := "…"` lines | a macro (Step 78): the steps in order |
| `Sleep 200` (in a block) | `delay` |
| `TrayTip "text", "title"` | `notify` |
| `WinMaximize "A"`, `WinMinimize "A"`, `WinRestore "A"`, `WinActivate "ahk_exe Code.exe"` | `window` |
| `A_Clipboard := "text"` | `clipboard` `set` |
| `::btw::by the way`, `:*:;sig::…`, options `* ? C C1 O B0 R T` | a hotstring trigger with `type_text` (`{Enter}`/`{Tab}` as characters) |
| `#HotIf WinActive("ahk_exe Code.exe")` … `#HotIf` | `when.apps` on the hotkeys in between |
| `#Requires AutoHotkey v2`, `#SingleInstance`, `#Warn`, `#NoTrayIcon` | nothing to convert |

Key names follow AutoHotkey's (`Esc`, `PgUp`, `AppsKey`, `Numpad0`, `MButton`, `XButton1`, …) and
become the hotkey contract's (`Escape`, `PageUp`, `ContextMenu`, `Numpad0`, `MouseMiddle`,
`MouseX1`, …).

## 3. What needs manual migration (and why)

| AutoHotkey v2 | Why Ddalgak does not convert it |
| --- | --- |
| `#c::…` (Win shortcuts) | Win shortcuts belong to Windows; Ddalgak does not take them (choose another key) |
| `a & b::…` (custom combinations) | no equivalent |
| `LButton`/`RButton` hotkeys | the primary buttons are never triggers (you could not click) |
| functions, variables, expressions, objects, classes | Ddalgak rules are data, not a language (matrix §8) |
| `DllCall`, `FileAppend`/`FileDelete`/…, `Download`, `RegWrite`, `ComObjCreate`, `RunWait`, `ExitApp`, `Reload` | side effects Ddalgak does not imitate |
| `Run "tool.exe"` by name | Ddalgak opens programs by their full path (choose the program in the editor) |
| `Send "{Shift down}…"`, `{Key 21}` and more | Ddalgak sends whole key presses; repeats are bounded |
| `Send` with `#` (Win) | no Win shortcuts from a string |
| `:X:` hotstrings and hotstrings that send shortcuts | a hotstring types text |
| other `#HotIf` expressions (`WinActive("Title")`, `GetKeyState`, variables) | Ddalgak matches programs, not titles or expressions; the hotkeys below it are listed too |
| `#Include`, other directives | another file is never read |
| `WinActive("title")`, window classes | windows are found by program only |

Each such line appears in the report with its line, column and reason. The rest of the script still
imports.

## 4. After the import

1. Open the new profile in the editor; each rule says what it does in a sentence.
2. Try them in the **Playground** (Step 76): pick the profile, a mode, a program, and press the keys —
   nothing is typed or opened unless you choose *Live*.
3. Turn rules on one by one; Ddalgak asks for permission for typing, launching or windows.
4. Switch to the profile when ready (Profiles → *Switch…*, with a side-by-side diff).

## 5. What Ddalgak adds to an AutoHotkey workflow

- **Visual editing**: create, edit, reorder, enable and delete rules without a script; Korean and
  English; keyboard and screen-reader friendly.
- **A safe playground**: see which rule fires for a key in a profile, mode and program, with every
  candidate explained, before anything is active.
- **Conflicts explained**: duplicate triggers, reserved keys and collisions with other Kim tools are
  shown with their reasons and non-destructive offers.
- **Modes and profiles**: layers, one-shot and timed modes, profiles with history and rollback.
- **Explicit consent**: typing, launching, the clipboard and window control are allowed per rule,
  and imported rules never carry consent.
- **Headless and agent-friendly**: every action has a CLI with JSON output and documented exit
  codes, for scripts and AI agents.

## 6. What Ddalgak does not claim

Ddalgak does not implement the AutoHotkey language or runtime, does not run `.ahk` files, does not
promise identical timing or edge-case behavior, and does not import AutoHotkey v1 scripts. The
[matrix](DDALGAK_AHK_MATRIX.md) states, feature by feature, what is supported, partial, planned or
not intended.

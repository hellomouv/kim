# Subak state and configuration schemas v1 (draft)

Status: **draft contract** (`config.toml`'s `[appearance]`, `[terminal]` and `[[profiles]]`
implemented in Step 20, [ADR 0051](../adr/0051-subak-settings-profiles-and-appearance.md)) ·
Step 03 · Implemented in Steps 10/29 (state) and 20 (config) ·
Decision: [ADR 0006](../adr/0006-state-and-configuration-storage.md)

## 1. `state.json` (schema `subak.state`, version 1)

**Implemented in Step 29 as schema version 2** ([ADR 0063](../adr/0063-durable-terminal-state.md),
[RECOVERY.md](../RECOVERY.md)); machine-readable:
[schemas/subak-state-v2.schema.json](schemas/subak-state-v2.schema.json), example
[examples/subak-state-v2/](examples/subak-state-v2/one-terminal-two-windows.json). Version 2 adds
`restore` per Terminal, window `size`, layout `cells`, pane `command_line`, `command_withheld`,
`env` as **names only**, `env_unset` and `history_limit`, and drops `env_secret_refs` (Step 34
decides secret references). Version 1 below is the Step 03 draft; it is migrated on load.

Machine-readable: [schemas/subak-state-v1.schema.json](schemas/subak-state-v1.schema.json);
examples in [examples/subak-state-v1/](examples/subak-state-v1/two-terminals.json), validated
by `cargo xtask schema-check`.

```json
{
  "schema": "subak.state",
  "schema_version": 1,
  "written_at": "2026-11-03T12:00:00Z",
  "writer": { "version": "0.1.0" },
  "crc32": "1c291ca3",
  "terminals": [ Terminal ]
}
```

| Object | Fields |
| --- | --- |
| Terminal | `id` (`t-` + 12 hex), `name`, `created_at`, `profile` (string or null), `restore` (`ask` \| `auto` \| `never`), `remain_on_exit` (bool), `active_window` (window id), `windows` (≥ 1) |
| Window | `id`, `index` (≥ 0), `name`, `automatic_name` (bool), `active_pane`, `layout` (Layout), `panes` (≥ 1) |
| Layout | `{ "type": "pane", "pane": "p-…" }` or `{ "type": "split", "direction": "horizontal" \| "vertical", "children": [{ "size": 0–1, "cells"?: ≥ 1, "node": Layout }] (≥ 2) }` — implemented in IPC since Step 22 (ADR 0056; `horizontal` = side by side); persisted with Step 29 |
| Pane | `id`, `index`, `cwd`, `command` (`{program, args}` or null = profile default), `restart_command` (same shape or null; **opt-in**, never derived), `env` (non-secret overrides), `env_secret_refs` (name → credential reference), `title` (or null), `claude` (`{session_id, resume}` or null), `scrollback` (`{persisted, file?}`) |

**Never stored:** PIDs (except as volatile hints outside this file), terminal content (unless
scrollback persistence is enabled, and then in separate files), secret values, client
identities.

### Integrity and migration

- `crc32` is the CRC-32 (IEEE) of the document serialized canonically **without** the `crc32`
  field (sorted keys, no insignificant whitespace), as 8 lowercase hex digits.
- Load: parse → check `schema` → check `crc32` → migrate `schema_version` step by step (backup
  first) → validate. A newer `schema_version` than the reader knows → read-only,
  `state_from_newer_version`.

## 2. `config.toml` (version 1)

Human-edited; unknown keys preserved; invalid values fall back to defaults with an error that
names the key and line (`SUBAK-FR-046`).

**Implemented (Step 20)** — read by the window and the `subak config`, `profile` and `new`
commands, never by the Host; data model [subak-config-v1](schemas/subak-config-v1.schema.json)
(examples in `examples/subak-config-v1/`); rules, edits, backups and the user guide in
[SUBAK_GUI §13](../SUBAK_GUI.md) and [SUBAK_CLI §3.13](../SUBAK_CLI.md):

```toml
config_version = 1            # a newer value: read-only, never rewritten

[appearance]
theme = "auto"                # auto | system | kim-dark | kim-light | high-contrast
font_family = ""              # "" = the design system's chain; else tried first (≤ 31 chars)
font_size = 13                # 8–36 logical px
line_height = 1.2             # 1.0–2.0
padding = 8                   # 0–16 logical px
opacity = 100                 # 50–100 %
motion = "system"             # system | reduced | full
bold_is_bright = true

[terminal]
default_profile = ""          # a profile's name; "" = the Host's default shell
copy_on_select = false
smart_ctrl_c = false
alt_is_meta = true            # #98 T2: Alt+key sends ESC first
backspace = "del"             # #98 T2: del | ctrl-h | delete
bell = "off"                  # #98 T3: off | visual | audible
notify_command_seconds = 0    # #98 T3: 0–86400; 0: off
notify_exit = false           # #98 T3
focus_follows_mouse = false   # #98 T1
pane_labels = false           # #98 T1

[tabs]                        # #98 T1
close_buttons = true
cycle = true
title = "{name}"              # {name} {title} {program} {cwd}
on_exit = "keep"              # keep | close

[dropdown]                    # #98 T3
height = 40                   # 20–100 (percent)
hide_on_blur = true
right_click = "copy-paste"    # copy-paste | menu | paste | none
middle_click = "none"         # none | paste
word_separators = "…"         # ≤ 128 characters; default: Windows Terminal's set
repeat_time_ms = 500          # 0–2000; tmux repeat-time for Ctrl+B Ctrl+Arrow (Step 23); 0: off

[[profiles]]                  # ≤ 64; names unique ignoring case
name = "PowerShell"           # 1–64 characters, required
program = "powershell.exe"    # required; a path or a name on the pane's PATH
args = ["-NoLogo"]            # ≤ 256 strings
cwd = 'C:\src'                # absolute
env = { RUST_BACKTRACE = "1" }   # set; never secrets (Subak refuses to write them)
unset_env = ["HTTP_PROXY"]    # removed
```

A profile with any other field (`host`, `port`, … — SSH and other kinds are Step 33) is left
out with a problem. Backups: `backups\config-<UTC>.toml` before `reset --all` and `import`,
`backups\config-invalid-<UTC>.toml` before an invalid file is replaced, the newest 5 of each.

**Planned** (the draft below; kept as unknown keys until their Steps implement them):

```toml
config_version = 1

[host]
start_at_login = false        # opt-in (SUBAK-FR-002)
idle_exit_seconds = 10

[restore]
mode = "ask"                  # ask | auto | never — auto applies only to Terminals with restore = "auto"
claude_resume = false         # opt-in (SUBAK-FR-045)

[scrollback]
lines = 10000
history_bytes_per_pane = 4194304
persist = false               # opt-in (SUBAK-FR-042)

[gui]
floating_icon = false
tray_icon = true
close_terminal_shortcut = "kill"   # kill | hide (ADR 0009)

[claude]
indicator = true
```

Key bindings are not part of `config.toml`. They live in `hotkeys.json` next to it, as for
every Kim tool ([HOTKEY_CONTRACT §8](../HOTKEY_CONTRACT.md), ADR 0017). This draft's former
`[keys]` table (`prefix`, `new_terminal`, …, `terminal_first_mode`) was removed before any
Subak release read it: the prefix is the leader `prefix`, and terminal-first mode is a preset
of disabled overrides (HOTKEY_CONTRACT §13.1).

Profiles (`[[profiles]]`) are the basic local ones above (Step 20); SSH, serial and other kinds
and per-profile appearance are Step 33. The draft `[gui]` keys were superseded: the floating icon
and tray choices are in `float.json` (Step 18), and the `Ctrl+W` behaviour is a hotkey binding.

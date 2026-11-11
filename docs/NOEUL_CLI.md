# Noeul CLI, agents and Kim

- **Status:** current (Step 62, 2026-11-12). Decision: [ADR 0076](adr/0076-noeul-agents-and-kim.md).
  Contract: [CLI_CONTRACT](CLI_CONTRACT.md) (envelope, exit codes, the Noeul error table);
  machine-readable command list: `noeul capabilities --json`. Requirements: `NOEUL-FR-110`…`113`,
  `NOEUL-SEC-010`.

Every command prints human text by default and exactly one JSON envelope with `--json`
(`{schema_version, command, ok, data | error, warnings}`); data goes to stdout, diagnostics to
stderr. Commands that change something take `--dry-run`; commands that remove something ask on a
terminal or need `--yes` (and fail with `confirmation_required`, exit 7, when nobody can answer:
`--no-input` or `KIM_NO_INPUT=1`).

## 1. Three kinds of commands

| Kind | Needs | Commands |
| --- | --- | --- |
| **Headless** — never opens a window, a tray icon or a prompt, needs no desktop, starts nothing | nothing | `status`, `capabilities`, `version`, `health`, `hotkeys list\|show\|set\|disable\|enable\|reset\|check\|export\|import`, `config show\|set\|reset`, `mcp tools`, `history clear` and `pin backup status\|clear` when Noeul is not running |
| **Instance** — talks to the running Noeul over a per-user pipe; never starts it unless `--start` is given | a running Noeul (`noeul_not_running`, exit 3, otherwise) | `pin list\|hide\|show\|close\|reopen\|set\|group …\|restore\|backup save`, `pin <file>`, `pin --clipboard`, `history list\|pin\|forget\|clear`, `capture --region\|--monitor\|--window … --pin\|--output`, `capture --cancel`, `quit`, `interop invoke`, `mcp serve` (its tools) |
| **Desktop** — shows something on the user's screen | an unlocked interactive desktop (`desktop_unavailable`, exit 6, when locked, on the secure desktop, disconnected or without a display) | `start`, `run`, `capture --interactive` (the F1 overlay), `settings` (the settings window) |

In a locked or remote-disconnected session nothing captures: `capture` fails with
`desktop_unavailable`; Noeul never opens a UI to work around it and never captures the secure
desktop (`NOEUL-SEC-003`).

## 2. Captures and the clipboard without the keyboard (`[privacy]`)

| Who asks | Setting | Default | Effect |
| --- | --- | --- | --- |
| The CLI, a script, `kim tools invoke` | `privacy.cli` | `notify` | each capture or clipboard read shows a notice; `allow`: silently; `deny`: refused with `capture_not_allowed` (exit 7) before anything is captured or read |
| An agent through `noeul mcp serve` | `privacy.mcp_captures` | `off` | refused with `capture_not_allowed`; `on`: allowed, always with the notice. Agents never read the clipboard. |

`noeul config set privacy.cli deny`, `noeul config set privacy.mcp_captures on` (the user's
decision; an agent must not run it for them).

## 3. Capture

```powershell
noeul capture --region 0,0,800,600 --pin            # pinned in place at 100 %
noeul capture --monitor primary --output C:\Shots\screen.png   # a PNG file (no metadata)
noeul capture --window active --output C:\Shots\w.png --replace
noeul capture --region 0,0,8,8 --pin --dry-run      # checks the region and the desktop only
```

`--output` takes an absolute path on a local drive ending in `.png` (relative paths are made
absolute from the current folder); an existing file is never replaced without `--replace`; the JSON
answer says `"file": {"written": true, "bytes": …}` and never echoes the path. `--clipboard` is
planned.

## 4. Agents: the MCP adapter

`noeul mcp serve` speaks MCP over stdio for an agent the **user** configured; it never starts
Noeul and has no network listener. Reading is always available; anything more needs a grant in the
client's configuration:

| Tool | Grant | Does |
| --- | --- | --- |
| `noeul_status` | — | running, pins, monitors, shortcut registrations |
| `noeul_pin_list` | — | the open pins (ids, rectangles, zoom, visibility) |
| `noeul_history_list` | — | the kept captures (numbers, rectangles, times) |
| `noeul_pin_file` | `pin` | pins a local image file without focus |
| `noeul_capture_to_file` | `capture` | a region or monitor to a PNG file — **also** needs `privacy.mcp_captures on` |

```json
{ "mcpServers": { "noeul": { "command": "noeul.exe", "args": ["mcp", "serve", "--allow", "pin"] } } }
```

`noeul mcp tools [--allow …] --json` shows what a configuration offers. Every call is appended to
`%LOCALAPPDATA%\mouv\noeul\mcp-audit.jsonl` (time, tool, outcome; never a path or pixels).

## 5. Kim

- **Install, update, uninstall**: `kim install noeul` (the package `cargo xtask package noeul`
  builds: `noeul.exe`, `capabilities.json`, licenses), `kim update noeul`, `kim uninstall noeul`.
  Noeul never needs Kim at run time and never updates itself.
- **Interop** (`kim tools invoke …`, [INTEROP](INTEROP.md)): `pin.list` (read), `image.pin`
  (write: `{path}`), `screen.capture-file` (write: `{region | monitor, output, replace}`; `privacy.cli`
  applies). `noeul interop invoke` answers one request from stdin, headless; it never starts Noeul.

## 6. Exit codes and errors

The shared codes and the Noeul table are in [CLI_CONTRACT §6](CLI_CONTRACT.md). The ones an agent
meets most: `noeul_not_running` (3), `pin_not_found` (3), `history_entry_not_found` (3),
`monitor_not_found` (3), `window_not_found` (3), `usage` (2), `capture_not_allowed` (7),
`confirmation_required` (7), `desktop_unavailable` (6), `clipboard_unavailable` (6).

## 7. Verified with a real agent

Windows 10 Pro 10.0.19045, Claude Code 2.1.322, Step 62: `claude -p … --mcp-config <file>
--strict-mcp-config --allowedTools mcp__noeul__noeul_status,mcp__noeul__noeul_capture_to_file`
started `noeul mcp serve --allow capture` against a sandboxed debug Noeul on its synthetic desktop
(off-screen, global hotkeys disabled). `noeul_status` answered (two monitors, `privacy.mcp_captures`
off); `noeul_capture_to_file` was refused with `capture_not_allowed` and no file was written; the
agent reported both and how the user could allow captures. The audit file recorded
`noeul_status ok` and `noeul_capture_to_file capture_not_allowed`, without a path.


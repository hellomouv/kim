# Claude Code integration

Status: **current** (Step 27: hooks and session events) · Owner: mouv · Decisions:
[ADR 0010](adr/0010-claude-code-integration.md), [ADR 0061](adr/0061-claude-code-hooks-and-session-events.md)

Subak can tell which pane runs Claude Code and what that session is doing — **only if you opt
in** by installing Subak's hooks into your Claude Code settings. It uses Claude Code's official
hooks; it never reads the screen, transcripts or prompts.

## 1. Opt in and out

```console
> subak claude hooks status                 # ~/.claude/settings.json by default
> subak claude hooks install --dry-run      # what would be added
> subak claude hooks install                # merges; writes settings.json.subak-backup-<time> first
> subak claude hooks uninstall              # removes exactly Subak's entries (another backup)
> subak claude hooks install --settings .claude\settings.local.json   # one project only
```

- Your other settings and hooks stay as they are, in their order; Subak's group comes after
  yours. The file is rewritten with two-space indentation and its own line ends.
- An invalid settings file is refused (`config_invalid`) and left unchanged.
- The hook command is this `subak.exe` by full path. After moving Subak, run `install` again.

## 2. What is sent, and what never is

| Hook event | Subak forwards | State |
| --- | --- | --- |
| `SessionStart` | session id, `startup`/`resume`/`clear`/`compact` | `idle` |
| `UserPromptSubmit` | session id | `in-progress` |
| `PostToolUse` | session id | `in-progress` |
| `Notification` | session id, `permission`/`idle`/`attention` | `need-input` (`done` for idle) |
| `PermissionRequest` (Step 28) | session id, `permission` | `need-input` |
| `Stop` | session id | `done` |
| `StopFailure` (Step 28) | session id, `api_error` (never the error text) | `error` |
| `SessionEnd` | session id, `clear`/`logout`/`exit`/`other` | session ends |

**Never sent, logged or stored:** prompts, tool input or output, notification messages,
`cwd`, `transcript_path`, file contents. The reporter prints nothing and always exits 0, so a
missing Host or a non-Subak terminal never disturbs Claude.

## 3. Which pane, and who may say so

Every Subak pane has `SUBAK_PANE_ID`, `SUBAK_TERMINAL_ID` and `SUBAK_ENDPOINT` in its
environment; Claude Code and its hooks inherit them. The Host accepts a report for a pane only
from a process **inside that pane's job** — a forged `SUBAK_PANE_ID` elsewhere is refused.

```console
> subak claude status            # every pane with a session
> subak claude status work:0.1   # one pane (or a window, or a Terminal)
```

Several Claude sessions in one pane are tracked separately; the latest one speaks for the pane.

## 4. Where you see it (Step 28)

Each tab gets a mark and the title says the state in words; the floating icon and the manager
show the most urgent state (need-input > error > done > in-progress > unknown > idle). A tool
call finishing never counts as `done`; only `Stop` does. `terminal.claude_indicators = false`
in `config.toml` hides the tab marks ([SUBAK_GUI §18](SUBAK_GUI.md)).

## 5. Honest fallbacks

- **No hooks installed / not a Claude pane:** no session — Subak never guesses from screen text.
- **An event Subak does not know** (a newer Claude Code): the state does not change.
- **No hook for 30 minutes while `in-progress`** (a missed hook, a killed Claude): `unknown`,
  announced by itself (the tab changes without a new hook).
- **An older Claude Code without `StopFailure`/`PermissionRequest`:** those states come from
  `Notification` or not at all; never an invented `error`.
- **The pane ends:** its sessions end.
- **The Host restarts:** sessions are in memory only (`persisted: false`) and start empty; the
  next hook re-creates them. Persisting the last session id for `claude --resume` is Step 29/30.

## 6. Versions checked

| Claude Code | How | Result |
| --- | --- | --- |
| 2.1.322 (Windows, native installer) | a real `claude -p` in a Subak pane with `--settings <file written by subak claude hooks install> --setting-sources local` (2026-11-11) | `SessionStart` → `idle`, `UserPromptSubmit` → `in-progress`, `Stop` → `done`, `SessionEnd` ended it; the answer printed normally |

Hook fields were checked against the official reference
(<https://code.claude.com/docs/en/hooks>, 2026-11-11). Automated tests use a stand-in that
feeds the same hook JSON in a real pane (`crates/subak/tests/claude_hooks.rs`); they do not
start Claude Code (it needs an account and network).

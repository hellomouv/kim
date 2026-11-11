# Subak recovery model

Status: **current** for persistence (Step 29) and restore with Claude resume (Step 30) ·
shutdown, sleep and crash validation: Step 31 · Decisions: [ADR 0006](adr/0006-state-and-configuration-storage.md),
[ADR 0063](adr/0063-durable-terminal-state.md), [ADR 0064](adr/0064-cold-recovery-and-claude-resume.md)

## 1. Two different things

| | Live survival | Cold restore |
| --- | --- | --- |
| When | a window or client closes, a client crashes | after a reboot, a Host crash or a stop |
| What survives | the processes themselves (the Host owns them) | only **records**: names, ids, windows, panes, layouts, working directories, configured commands |
| Since | Steps 09–14 | Step 29 keeps the records; Step 30 restarts from them |

A reboot ends every Windows process. Nothing can bring a program's memory back; Subak can only
start configured commands again — and only when the user asked for it (`SUBAK-SEC-002`).

## 2. What is saved (Step 29)

`state.json` in `%LOCALAPPDATA%\mouv\subak\state\` (or the Host's `--state-dir`), schema
`subak.state` v2 ([STATE_SCHEMA](contracts/STATE_SCHEMA.md)): Terminals, windows, panes,
layouts, working directories, the start command (not when it looks secret), the names of
environment overrides, history limits, restore policy, the latest Claude session id. Never:
PIDs, screen content, environment values, secrets.

Written 500 ms after a change (coalesced), atomically, and once more at a clean stop; backups
`backups\state-<time>.json` (newest 5).

## 3. What happens on load

1. A valid file: its Terminals become **saved records**. Nothing is started.
2. A corrupt file: kept as `state.corrupt-<time>.json`; the newest valid backup is used.
3. A file from a newer Subak: read-only, never rewritten.
4. An older schema: migrated after a backup.

## 4. Restoring (Step 30)

```console
> subak recover list                         # restorable Terminals, pane by pane (starts nothing)
> subak recover work --dry-run --claude-resume
> subak recover work --yes                   # the same id, windows and layout, fresh shells
> subak recover agent --claude-resume --yes  # claude --resume <saved id> in its pane
> subak recover job --with-commands --yes    # run the saved start commands again (explicit)
> subak state policy work auto               # restored at the next Host start, shells only
```

| What | Comes back |
| --- | --- |
| Terminal id, name, windows, layout and sizes, active pane | yes |
| A shell in each pane's folder | yes (the profile folder if the folder is gone) |
| The Claude conversation | with `--claude-resume`, when Claude Code still has the session and it does not already run |
| Saved start commands | only with `--with-commands` (never one withheld as secret-looking) |
| Running programs, their memory, child processes, shell variables and history, screen contents | **no** — a reboot or stop ended them |

A second restore of the same Terminal is refused, and so is one whose name a live Terminal
uses.

## 5. Recovery limits

- Changes in the last 500 ms before a crash can be lost; the file is never torn.
- Commands that looked secret and environment values are not restorable (on purpose).
- A missing working directory or program is reported by `subak state show`/`check`; nothing is
  started in its place.

## 6. Commands

```console
> subak state show            # what is saved (nothing changes)
> subak state check           # exit 4 state_corrupt, 5 state_from_newer_version
> subak state backups
> subak state policy work never --dry-run
> subak state forget old-terminal
> subak host stop; subak state rollback   # the newest valid backup becomes current
```

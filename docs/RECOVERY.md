# Subak recovery model

Status: **current** for persistence (Step 29) · restore and Claude resume: Step 30 · shutdown,
sleep and crash validation: Step 31 · Decisions: [ADR 0006](adr/0006-state-and-configuration-storage.md),
[ADR 0063](adr/0063-durable-terminal-state.md)

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

## 4. Recovery limits

- Changes in the last 500 ms before a crash can be lost; the file is never torn.
- Commands that looked secret and environment values are not restorable (on purpose).
- A missing working directory or program is reported by `subak state show`/`check`; nothing is
  started in its place.

## 5. Commands

```console
> subak state show            # what is saved (nothing changes)
> subak state check           # exit 4 state_corrupt, 5 state_from_newer_version
> subak state backups
> subak state policy work never --dry-run
> subak state forget old-terminal
> subak host stop; subak state rollback   # the newest valid backup becomes current
```

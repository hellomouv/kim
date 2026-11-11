# ADR 0063 — Durable Terminal state: saved records that never start by themselves

- **Status:** Accepted (Step 29, 2026-11-11)
- **Requirements:** `SUBAK-FR-040`, `SUBAK-FR-046`, `SUBAK-SEC-001`, `SUBAK-SEC-002`,
  `SUBAK-SEC-003`, `SUBAK-SEC-011`, `KIM-SEC-013`
- **Implements:** [ADR 0006](0006-state-and-configuration-storage.md) (locations, writing rules)

## Decision

1. **Who writes.** The Host writes `state.json` (`subak.state`, schema version **2**) in its
   state folder: `%LOCALAPPDATA%\mouv\subak\state\` for the user's default endpoint, or
   `--state-dir <dir>` (`host start`, `host serve`). A Host on a private endpoint without
   `--state-dir` persists nothing (tests and experiments never touch the user's state;
   `state.status.enabled` says so).
2. **What.** Per Terminal: id, name, created time, restore policy (`ask` default, `auto`,
   `never`), `remain_on_exit`, the active window; per window: id, index, name, automatic name,
   active pane, size and the layout tree (`cells` included); per pane: id, index, cwd, the
   command and verbatim command line **unless anything in them looks secret** (then both are
   null and `command_withheld: true`), the *names* of environment overrides (never values),
   `history_limit`, the latest Claude session id (`resume: false`; opt-in is Step 30).
   **Never:** PIDs, screen or scrollback content, environment values, client identities, hook
   input.
3. **When.** Any change of Terminals, windows, panes or Claude references marks the state
   dirty; a writer thread waits 500 ms (coalescing everything in between) and writes once.
   Nothing is written while nothing changes. A clean stop writes once more and then *freezes*
   the state: ending the panes does not erase the Terminals that should survive. A Terminal the
   user ends (kill, exit) disappears from the next write.
4. **How.** `state.json.tmp` → flush → `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`; a
   crash leaves the old or the new file. Each Host start copies the current file to
   `backups\state-<time>-start.json`; edits by the CLI and migrations copy it first too; the
   newest 5 backups stay. Files over 32 MiB are refused.
5. **Load.** parse → `schema` → CRC-32 of the canonical body → migrate (v1 → v2: defaults, env
   values dropped; backup first) → structure check (unique ids, a window and a pane each, the
   layout naming exactly the window's panes). Corrupt: kept as `state.corrupt-<time>.json`, the
   newest valid backup is used and written back. Newer version: read-only for the Host and the
   CLI, never rewritten (`state_from_newer_version`).
6. **Loading starts nothing.** Terminals of an earlier run become *saved records*: listed by
   `state.status` and `subak state show`, written back unchanged, never started. Step 30
   restores them (with `ask`/`auto`/`never` and explicit opt-ins); until then only the user can
   start a Terminal.
7. **Control.** `subak state show|check|backups` read the file and change nothing;
   `subak state forget <saved>` and `subak state policy <terminal> <ask|auto|never>` go through
   the running Host (it owns the file) or, without one, edit the file (backup, atomic write);
   `subak state rollback [<backup>]` makes a backup current — refused while a Host runs, the
   replaced file kept as `state.before-rollback-<time>.json`. All changes take `--dry-run`.

## Consequences

- Recovery limits: changes in the last 500 ms before a crash or a killed Host can be lost
  (the previous write stays intact); a pane's environment values and secret-looking commands
  are not restorable by design; scrollback is not persisted (opt-in is a later Step).
- GUI geometry stays in the window's own preferences (`gui.json`, Step 16).
- Two Hosts with the same `--state-dir` would overwrite each other; the default folder belongs
  to the default endpoint's single Host.

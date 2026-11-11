# ADR 0064 — Cold recovery and Claude resume: explicit, shells first, once

- **Status:** Accepted (Step 30, 2026-11-11; the GUI recovery flow is not built yet)
- **Requirements:** `SUBAK-FR-040`, `SUBAK-FR-045`, `SUBAK-SEC-002`, `SUBAK-SEC-003`
- **Builds on:** [ADR 0063](0063-durable-terminal-state.md) (saved records),
  [ADR 0010](0010-claude-code-integration.md) §5, [ADR 0061](0061-claude-code-hooks-and-session-events.md)
- **Checked against:** Claude Code 2.1.322 `claude --help`: `-r, --resume [value]` resumes a
  conversation by session id

## Decision

1. **States.** A saved record is `restorable` (`restoring` while it is being restored); once
   restored the Terminal is live (`running`, later `stopped`/`failed` as before). A restored
   Terminal is never presented as the old processes: its panes are **new processes with new
   pane ids**. It keeps its **Terminal id**, name, creation time, windows (index, name when set,
   order), the layout tree with the saved cell sizes, and the active window and pane.
2. **What starts.** By default each pane gets a **fresh shell** (the Host's default shell) in its
   saved folder (the profile folder when that folder is gone, with a note). Saved start commands
   run again only with `with_commands` (`subak recover … --with-commands`), and a command that
   was withheld as secret-looking cannot run (a note says so).
3. **Claude resume** (opt-in: `--claude-resume`, or `subak state policy <t> <policy>
   --claude-resume` stored per Terminal for later restores): a pane with a saved Claude session id
   starts `cmd.exe /d /k claude --resume <id>` — the official resume flag — inside a shell, so a
   failed resume leaves a usable shell. Before that Subak checks that Claude Code still has the
   session (`<CLAUDE_CONFIG_DIR or ~/.claude>/projects/*/<id>.jsonl` exists; only the file name
   is looked at) and that no live pane already reports it; otherwise a shell starts with a note.
   Subak never sends a prompt or approves anything: the resumed Claude waits for the user with
   the user's own Claude settings. The last known session id of a pane is kept after the session
   ends, so a finished conversation can be resumed too.
4. **Explicit and once.** `subak recover list` and `recover <t> --dry-run` start nothing and say
   what cannot come back (the programs and their memory, unsaved shell state, screen contents,
   child processes). `recover <t>` needs `--yes`. A record being restored, or a Terminal whose id
   or name is live, is refused (`invalid_state` / `name_conflict`): no duplicate Terminals, no
   repeated side effects.
5. **`auto`.** At Host start, records with `restore = auto` are restored at once — shells only,
   plus a Claude resume where the user stored the opt-in. `ask` (the default) and `never` wait
   for the user.

## Consequences

- A cold restore brings back the workspace's shape, not its processes; running jobs must be
  started again by the user (or by `--with-commands`, by explicit choice).
- The window shows restored Terminals as ordinary tabs once restored; a recovery prompt in the
  window is a follow-up (the CLI and IPC flow is complete).
- Real reboots and sleep are validated in Step 31.

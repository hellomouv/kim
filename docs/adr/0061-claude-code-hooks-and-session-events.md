# ADR 0061 — Claude Code hooks and session events: opt-in merge, job-verified reports

- **Status:** Accepted (Step 27, 2026-11-11)
- **Requirements:** `SUBAK-FR-050`, `SUBAK-FR-051`, `SUBAK-FR-052`, `SUBAK-SEC-002`, `KIM-SEC-013`
- **Refines:** [ADR 0010](0010-claude-code-integration.md) §1–§4, §6
- **Checked against:** Claude Code **2.1.322** (installed on the development PC) and the official
  hooks reference, <https://code.claude.com/docs/en/hooks> (read 2026-11-11)

## Context

ADR 0010 chose Claude Code's official hooks over screen text. Step 27 builds the integration and
must coexist with the user's own Claude settings and hooks, support several Claude sessions,
keep prompts and tool content private, and stay honest when hooks are missing.

## Decision

1. **Events.** Subak listens to `SessionStart` (`source`), `UserPromptSubmit`,
   `PostToolUse` (matcher `*`), `Notification` (`notification_type` / `message`), `Stop` and
   `SessionEnd` (`reason`). Every hook input has `session_id` and `hook_event_name`; nothing
   else is required. `PreToolUse` is not installed (one process per tool call for no extra
   state).
2. **Opt-in install that preserves the user's settings.** `subak claude hooks install
   [--settings <file>]` (default `%USERPROFILE%\.claude\settings.json`):
   - reads the file with an **order-preserving JSON reader** (keys, strings and numbers kept
     exactly as written; the file is re-indented with two spaces, its line ends kept);
   - adds one hook group per event *after* the user's groups, command
     `"<full path of subak.exe>" claude report`, `timeout` 10 s; an event that already has
     Subak's hook is left alone (idempotent);
   - writes a byte-exact backup `<file>.subak-backup-<unix time>[-n]` first (never over an
     earlier backup), then the file **in place** (its ACL stays); a missing file is created;
   - refuses a file that is not valid JSON (`config_invalid`) and changes nothing;
   - `uninstall` removes exactly the entries whose command runs `subak … claude report`, and the
     groups and event lists that leaves empty; `status` reports per event; `--dry-run` plans.
   Nothing is installed automatically, and `KIM-SEC-001` holds: Subak never installs or updates
   Claude Code.
3. **The reporter** (`subak claude report`) reads up to 4 MiB of stdin (the rest is drained),
   takes `session_id` and `hook_event_name` (letters, digits, `-`, `_` only) and a coarse reason
   (`startup|resume|clear|compact|other`, `permission|idle|attention`, `clear|logout|exit|other`),
   and sends `claude.report {pane_id, session_id, event, reason}` as a `hook` client to the Host
   named by `SUBAK_ENDPOINT` (set in every pane since this Step, with `SUBAK_PANE_ID`). Prompts,
   tool input and output, messages, `cwd` and `transcript_path` are never sent, logged or
   stored. It prints nothing without `--json` (Claude adds some hooks' stdout to the
   conversation), waits at most 0.5 s to connect and 2 s for the answer, and **always exits 0**.
   Outside a Subak pane, or without a running Host, it does nothing.
4. **Authentication.** The Host believes a report for pane `p-…` only when the reporting
   connection's process (the pid the OS reports for the pipe client) runs **in that pane's job**
   (`IsProcessInJob`). Claude Code and its hook commands inherit the job; a process elsewhere —
   even of the same user, even with a forged `SUBAK_PANE_ID` — gets `unauthorized` and is counted
   (`host.stats` `ipc.claude_refused`). The pipe itself is per-user (ADR 0019).
5. **Sessions and states** (ADR 0010 §4) are kept per pane and per `session_id`, in the Host's
   memory only (`persisted: false`): `SessionStart` → `idle`, `UserPromptSubmit`/`PostToolUse`
   → `in-progress`, `Notification` permission/attention → `need-input`, idle prompt or `Stop` →
   `done`, `SessionEnd` ends the session, a pane that ends ends all of its sessions. An event
   Subak does not know (a newer Claude Code) keeps the state; a session `in-progress` without a
   hook for 30 minutes shows `unknown` (`stale: true`). No hooks means no sessions: Subak never
   infers Claude from screen text or process names. Several sessions in one pane (a nested
   `claude`) are separate; the most recently reported one speaks for the pane.
6. **API.** `claude.report` (hook clients), `claude.status {target?}` (cli, gui, mcp) and the
   event `claude.state_changed` on topic `claude`; CLI `subak claude status [target]`.

## Consequences

- Accurate states need the user's opt-in; without it Subak shows nothing rather than a guess.
- The installed hook names this `subak.exe` by path: after moving Subak, run `install` again
  (`status` shows the command in use; old entries are recognized and replaced by `uninstall`).
- Persisting the last `session_id` per pane for `claude --resume` is Step 29/30; the GUI
  indicators are Step 28.
- If Claude Code renames events or fields, reports stop matching and panes fall back to no
  state or `unknown`; `docs/CLAUDE_CODE_INTEGRATION.md` lists the versions checked.

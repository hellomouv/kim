# ADR 0062 — Claude Code status: one state machine, one priority, marks with words

- **Status:** Accepted (Step 28, 2026-11-11; the visible window pending the maintainer)
- **Requirements:** `SUBAK-FR-050`, `SUBAK-FR-051`, `SUBAK-FR-052`, `SUBAK-FR-053`
- **Builds on:** [ADR 0010](0010-claude-code-integration.md), [ADR 0061](0061-claude-code-hooks-and-session-events.md)
- **Checked against:** the official hooks reference, <https://code.claude.com/docs/en/hooks>
  (2026-11-11): `PermissionRequest` runs when Claude is about to ask for permission;
  `StopFailure` runs instead of `Stop` when a turn ends on an API error.

## Decision

1. **States and evidence** (per session, in the Host):

   | State | Set by | Never set by |
   | --- | --- | --- |
   | `idle` | `SessionStart` | — |
   | `in-progress` | `UserPromptSubmit`, `PostToolUse` (a tool finished: the turn goes on) | — |
   | `need-input` | `PermissionRequest`, `Notification` (permission or attention) | — |
   | `done` | `Stop`, `Notification` idle prompt | a tool's end (`PostToolUse`, `PostToolUseFailure`) |
   | `error` | `StopFailure` (API error: rate limit, auth, …) | a failed tool call (Claude goes on) |
   | `unknown` | `in-progress` without any hook for 30 min (a lost `Stop`, a killed Claude) | — |
   | (ended) | `SessionEnd`, the pane ending | — |

   An unknown event keeps the state; a repeated state sends no event. Each session has the
   time of its last hook (`since`). The installer now adds `PermissionRequest` (matcher `*`) and
   `StopFailure`; the reporter still never answers a permission request (it prints nothing), so
   Claude asks as always.
2. **One priority** (`SUBAK-FR-051`): `need-input` > `error` > `done` > `in-progress` > `unknown`
   > `idle` > `none` — for a Terminal over its panes' sessions (`claude_state` in Terminal objects,
   `subak ls`/`info`), and for the floating icon over Terminals. Non-Claude panes add nothing.
3. **Events.** Besides `claude.state_changed` (topic `claude`), the Host sends
   `terminal.claude_changed {terminal_id, claude_state, previous}` on topic `terminals` when a
   Terminal's aggregate changes; the window and the manager, already subscribed, update the tab,
   the title, the row and the icon without listing again.
4. **Stale signals cost nothing while idle.** The Host's supervisor waits until the earliest
   deadline of an in-progress session (only while one exists), then announces `unknown` once.
   There is no polling and no timer otherwise.
5. **Marks with words.** A tab gets a mark after its status glyph — `!` need-input (warning),
   `✕` error (danger), `✓` done (success), `◔` in-progress (accent), `?` unknown, `◦` idle (muted)
   — a distinct shape per state, never colour alone; the title adds the words ("(Claude needs
   input)"), read by screen readers; the manager row and the icon's tooltip already say it
   (Step 18). Without a state there is no mark.
6. **The user decides.** `config.toml` `terminal.claude_indicators = false` (`subak config set`)
   hides the tab marks and the title words. Nothing but the state's name is ever shown.
7. **CLI.** `subak claude status [target]` and its alias `subak claude list` are read-only and
   answer what the GUI draws (`claude_state` of `subak ls` equals the tabs).

## Consequences

- `error` needs a Claude Code that has `StopFailure`; an older one never reports it and shows
  `done` or `unknown` instead — never a made-up error.
- A Host restart loses the states (memory only, ADR 0061); persistence is Step 29/30.

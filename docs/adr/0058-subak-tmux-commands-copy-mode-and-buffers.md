# ADR 0058 — Subak tmux commands, copy mode and paste buffers: one parser for the prompt and the CLI, Subak targets only, buffers in the Host's memory

Status: **Accepted** (Step 24; the visible window with a real keyboard pending the maintainer,
[SUBAK_GUI §16](../SUBAK_GUI.md)) · Date: 2026-11-11 · Builds on
[ADR 0056](0056-subak-pane-splits-and-layout.md) (the layout methods),
[ADR 0057](0057-subak-prefix-and-key-binding-engine.md) (the prefix table),
[ADR 0048](0048-subak-search-clipboard-and-links.md) (mark mode, search, the clipboard),
[ADR 0007](0007-object-identity-and-naming.md) (targets)

## Context

Step 24 asks for an interactive tmux command prompt with parseable commands (lists, creation,
kill, rename, select, split, join/break, resize, detach), a target syntax with stable errors and
quoting, a copy mode with scrollback, search, selection, buffers and paste, headless equivalents
with machine-readable output, and tmux reference behavior.

## Decision

1. **One parser** (`crates/subak/src/tmuxcmd.rs`) serves the window's prompt (`Ctrl+B :`) and
   `subak command` (alias `subak tmux`): tmux's quoting (`'…'`, `"…"` with `\\ \" \$ \n \t \e`,
   `\` escapes, an unquoted `;` between commands, `\;` literal, `#` comments), single-letter flags
   (`-hd`, `-tfoo`, `-t foo`, `--`), 30 commands with their tmux aliases (session, window, pane,
   layout, buffer, `send-keys`, `capture-pane`, `copy-mode`, `detach-client`). Unknown commands
   and flags, conflicting flags, bad values, unsupported tmux flags (with what to use instead) and
   missing arguments are errors with a stable `kind`; nothing is guessed.
2. **Targets are Subak's** (`<terminal>[:<window>[.<pane>]]` or an id). tmux's relative and
   special forms (`{last}`, `:+`, `.-`, `%3`, `@1`, `$0`, `!`, `~`, `=`) are refused with a hint.
   Without `-t` a command acts on its **context**: in the window, the shown Terminal (the Host's
   active pane *when the command runs*, so `split-window; select-pane -L` follows tmux); for
   `subak command`, `--target`, else the pane it runs in (`SUBAK_PANE_ID`, set in every pane).
3. **Execution** maps each command to one IPC request (or a window selection, or a window-only
   action); a line runs in order and stops at the first refusal. `kill-*` need `--yes` in the CLI;
   in the window the Host's busy rule decides and the window asks before `force`. `copy-mode` and
   `detach-client` are the window's own.
4. **New Host methods** (feature `terminal.buffers`): `pane.send_keys` (tmux key names to bytes,
   or literal text), `pane.break` (a pane leaves for a window of its own), `pane.join` (a pane
   joins another window as a split; never across Terminals), and `buffer.set|list|get|delete|
   paste`. Moving a pane keeps its process, screen and stream.
5. **Paste buffers live in the Host's memory only**: newest first, at most 50 (the oldest
   *automatic* buffer goes first; named ones stay), 1 MiB each, never written or logged, gone with
   the Host. Lists show a one-line, redacted preview; the text only on an explicit `show-buffer`.
   `paste-buffer` types the text as input with line ends as carriage returns (tmux without `-r`).
6. **Copy mode** reuses the view's keyboard selection (mark mode): `Ctrl+B [` starts at the
   cursor; arrows and tmux's vi keys move (`h j k l w b 0 ^ $ g G`, `PageUp/PageDown`,
   `Ctrl+U/Ctrl+D`); `Space` begins a selection (`v` toggles it); `/`, `?`, `n`, `N` use the search
   bar; `Enter` copies into a new paste buffer *and* the clipboard and leaves; `q`/`Esc` leave. No
   key reaches the program meanwhile. `Space`, `Enter`, `q`/`Esc` are the contract's `copy-mode`
   context actions (rebindable); the movement keys are fixed for now.
7. **Answers in the window**: `list-panes`, `list-windows`, `list-sessions`, `list-buffers`,
   `show-buffer` and `capture-pane` show their answer in the pane's place (the help view); errors
   are a banner notice.

## Consequences

- Scripts can drive Subak in tmux's words (`subak command 'split-window -h -l 30%'`), also from
  inside a pane without naming it.
- Not yet: `bind-key`/`unbind-key` as commands, formats (`-F`), `display-message`, `if-shell`,
  `run-shell`, `source-file`, `choose-*`, copy-mode emacs keys and rectangle selection, the
  `copy-mode` key table's other actions as rebindable actions (Steps 25+).

## Alternatives

- **A second, CLI-only syntax.** Rejected: one grammar for the prompt and scripts.
- **Resolve tmux's relative targets.** Rejected for now: `{last}` and friends need state the Host
  does not keep; guessing would act on the wrong pane.
- **Buffers in the window.** Rejected: tmux's buffers are the server's; a CLI and a second window
  must see the same ones.

# ADR 0059 — Subak advanced tmux behaviours: options, alerts, tokens, formats and hooks

- **Status:** Accepted (Step 25, 2026-11-11; the visible window and a real keyboard pending the
  maintainer)
- **Requirements:** `SUBAK-FR-013`, `SUBAK-FR-017`, `SUBAK-FR-018`, `KIM-FR-010`, `KIM-FR-012`
- **Extends:** [ADR 0056](0056-subak-pane-splits-and-layout.md) (layouts),
  [ADR 0057](0057-subak-prefix-and-key-binding-engine.md) (prefix table),
  [ADR 0058](0058-subak-tmux-commands-copy-mode-and-buffers.md) (command lines, targets)

## Context

Step 25 asks for tmux's advanced behaviours: layout presets, `synchronize-panes`, advanced
targets, hooks and events, options, a status line, activity/bell alerts, navigation and
scripting. The tmux 3.8 inventory ([TMUX_COMPATIBILITY.md](../TMUX_COMPATIBILITY.md)) had 353
rows targeted at Step 25 — more than one Step can build well. The Step must not claim parity it
has not measured.

## Decision

1. **A measured subset, an explicit inventory.** Step 25 builds the subset below with automated
   evidence; every other Step-25 row becomes `deferred` / `post-26` and names one follow-up group
   F1–F10 of [#97](https://github.com/madebymouv/kim/issues/97). Subak does not claim full parity.
2. **Layouts.** `main-horizontal`, `main-vertical` and their `-mirrored` forms with tmux's
   default `main-pane-height` 24 / `main-pane-width` 80 (clamped so the other panes keep their
   minimum); the main pane stays the main pane from one main preset to the next.
   `next-layout`/`previous-layout` follow tmux's order (even-horizontal, even-vertical,
   main-horizontal, -mirrored, main-vertical, -mirrored, tiled); `select-layout -E` gives every
   split equal parts. IPC `window.select_layout` takes `next`, `previous`, `spread` too.
3. **Targets.** tmux's tokens work **after a Terminal**: windows `work:` (active), `{last}` `!`,
   `{next}` `+`, `{previous}` `-`, `{start}` `^`, `{end}` `$`, `+N`/`-N` (wrapping); panes
   `work:1.{last}`, `.!`, `.+`, `.-`, `{active}` `@`, `{left-of}`/`{right-of}`/`{up-of}`/`{down-of}`
   (the layout's neighbour, wrapping as `select-pane -L` does) and the edge/corner tokens. A bare
   token (`{last}`, `:+`) and tmux's ids (`%3`, `@1`, `$0`) stay refused: Subak never guesses
   which Terminal is meant (ADR 0058). The Host keeps each Terminal's previous window and each
   window's previous pane (`last_window` in Terminal objects) for `last-window`/`last-pane`
   (`Ctrl+B l`, `Ctrl+B ;`).
4. **Window options** live per window in the Host (memory, like tmux): `synchronize-panes` (off),
   `monitor-activity` (off), `monitor-bell` (on), set with `setw`/`set -w` (`on`/`off`, or toggled)
   and IPC `window.set_option` (event `window.options`), read with `show-options` and in Window
   objects (`options`). `-g`, other scopes and other options are refused with a hint (F2).
   - `synchronize-panes`: interactive input of a client and `send-keys` reach every running pane
     of the window; `paste-buffer` does not (as in tmux).
   - Alerts: output (`monitor-activity`) or a BEL control (`monitor-bell`; the BEL ending an OSC
     string is not one) in a window **that is not shown** flags it once (`activity`, `bell`) and
     sends `window.alert {kind}`; showing the window clears its flags.
5. **Hooks.** `set-hook [-u] <name> '<tmux command line>'` / IPC `hook.set` keeps at most 32
   hooks in the Host's memory; `show-hooks` / `hook.list` lists them. Names: `after-new-window`,
   `after-split-window`, `after-select-pane`, `after-select-window`, `pane-exited`, `pane-created`,
   `window-created`, `window-closed`, `window-renamed`, `window-layout-changed`,
   `window-pane-changed`, `session-created`, `session-closed`, `session-renamed`,
   `session-window-changed`, `alert-activity`, `alert-bell` — each after the matching Host event.
   A hook runs **tmux commands only** (never a shell), on one Host thread, in the event's context
   (its Terminal, window, pane), never under the state lock. A hook may not contain `kill-*`
   (refused when set) and gives no consent: hooks never end processes. Events caused while a hook
   runs trigger no hooks (no loops). Results are counted in `host.stats` (`hooks_run`,
   `hooks_failed`), never shown.
6. **Formats.** `display-message [-p] [-t target] [format]` expands `#{name}`,
   `#{?name,yes,no}`, `#S #I #W #P #D #F ##` with the variables of ADR §10.2 that Subak has
   (`session_*`, `window_*`, `pane_*` subsets); unknown names expand to nothing, as in tmux.
   `-p` prints (CLI: method `format`, `{message}`); without it the window shows the text in its
   banner (`Ctrl+B i`). The CLI and the window expand formats from the Terminal's topology
   (`pane.list`); the Host has no format engine yet (F3).
7. **Status line.** The window title carries tmux's window list with flags (`{0:cmd* 1:logs-!}`:
   `*` shown, `-` last, `#` activity, `!` bell, `Z` zoomed) when a Terminal has several windows or
   an alert. A drawn status line inside the tab, styles and `status-*` options are F4.
8. **Keys.** New `@prefix` actions: `layout.main-horizontal` `Alt+3`, `layout.main-vertical`
   `Alt+4`, `layout.main-horizontal-mirrored` `Alt+6`, `layout.main-vertical-mirrored` `Alt+7`,
   `layout.next` `Space`, `layout.spread` `char:E`, `window.last` `char:l`, `pane.last` `char:;`,
   `window.info` `char:i` — tmux's keys.
9. **Not added:** a `subak events` CLI stream (IPC `events.subscribe` serves scripts and agents);
   linked windows (`link-window`) — Subak windows belong to one Terminal; the documented
   alternative is `join-pane`/`break-pane` within a Terminal and several clients on one Terminal
   (F6).

## Consequences

- Multi-client behaviour is unchanged: options, flags, hooks and the previous selection are Host
  state, so every client sees the same; the GUI refreshes on `window.alert` and `window.options`.
- Hook command lines are not persisted (tmux keeps them in the server too); Step 29 decides
  durable state.
- The matrix counts move: 81 implemented, 136 partial, 347 deferred, 17 planned, 25 unsupported.

# ADR 0057 — Subak's prefix and key binding engine: the contract's leader in the window and the console client, pass-through derived from the bindings, tmux repeat-time

Status: **Accepted** (Step 23; the visible window with a real keyboard and IME pending the
maintainer, [SUBAK_GUI §15](../SUBAK_GUI.md)) · Date: 2026-11-11 · Builds on
[ADR 0009](0009-gui-clients-and-keyboard-dispatch.md) (dispatch order, `Ctrl+B` prefix), the hotkey contract
([HOTKEY_CONTRACT](../HOTKEY_CONTRACT.md) §3.3 leaders, §5.3 sequences, §5.4 `repeat`, §13.1),
[ADR 0042](0042-subak-window-tabs-and-safe-close.md) (tab keys and safe close),
[ADR 0056](0056-subak-pane-splits-and-layout.md) (the layout methods the keys call)

## Context

Since Step 17 the window shipped the hotkey contract's single-chord actions only; the `prefix`
leader and its `@prefix …` table were in the final manifest but filtered out, and the console
client detached with a fixed `Ctrl+B d`. Step 23 asks for the prefix with tmux's table, a
dispatcher that keeps `Ctrl+Tab`/`Ctrl+T`/`Ctrl+W` (tabs) apart from internal window and pane keys
and from shell input, configurable bindings (also per profile) and timeouts, no interception in
unsafe states, keyboard help, and tests across layouts.

## Decision

1. **The leader ships.** The window's manifest is the final manifest filtered to what the build
   implements, now with the `prefix` leader and every `@prefix` default. The matcher resolves
   `@prefix` to the leader's current chord when it builds its table, so `"leaders": {"prefix":
   "Ctrl+A"}` moves the whole table and `Ctrl+B` becomes ordinary input. A leader is changed, never
   disabled (contract §8).
2. **The table** (scope `app`, every key rebindable): `c` new window, `n`/`p` next/previous
   window, `0`–`9` select a window, `,` rename the window in place, `&` kill the window, `%`/`"`
   split, arrows focus, `Ctrl+Arrow` (repeatable) / `Alt+Arrow` (by 5, repeatable) resize, `z`
   zoom, `x` kill the pane, `o` next pane, `{`/`}` swap, `Ctrl+O`/`Alt+O` rotate, `Alt+1`/`Alt+2`/
   `Alt+5` even-horizontal/even-vertical/tiled, `r` redraw, `d` detach, `$` rename the Terminal,
   `?` help, `Ctrl+B` a literal prefix. Thirteen of these (`o`, `{`, `}`, `Ctrl+O`, `Alt+O`,
   `Alt+1/2/5`, `Alt+Arrow`, `r`) are new actions added to the final manifest (§13.1). They act on
   the **shown Terminal** by id: no prefix key reaches another tab.
3. **Pass-through is derived, not configured** (contract §13.1): after the prefix, the keys of any
   single-chord `app` binding (`Ctrl+T`, `Ctrl+W`, `Ctrl+Tab`, …) go to the program. An unbound key
   after the prefix is swallowed with a hint; `Esc` or a click cancels; there is no timer.
4. **tmux `repeat-time`:** after a `repeat: true` prefix binding fires, its key — or another
   repeatable key of the table — fires again without the prefix for `terminal.repeat_time_ms`
   (`config.toml`, 0–2000, default 500; 0 off); any other key ends that.
5. **Unsafe states:** the IME composition guard comes first (no prefix while composing); modal
   parts (rename field, confirmation, question, keyboard help) take the keys; the search bar and
   mark mode take theirs first. A password prompt cannot be told apart in a terminal, as in tmux:
   the prefix behaves the same there (documented).
6. **Help:** `help.show` (`F1`, `Ctrl+B ?`) shows every binding as bound now, with the leader
   spelled out and its title, in the pane's place; arrows and Page keys scroll; `Esc` closes.
7. **Per profile:** `subak-gui --hotkeys-profile <name>` applies the `profiles` overlay of
   `hotkeys.json` for that run (until the Host records a Terminal's profile, Step 33).
8. **The console client** reads the same `hotkeys.json`: the prefix's byte (a `Ctrl+<letter>`
   leader) and the character of `client.detach`'s `@prefix char:<c>`; anything a console cannot
   send as one byte keeps `Ctrl+B d` with a notice. Its banner names the keys in use.
9. **Kill keys ask when busy.** `Ctrl+B x` and `Ctrl+B &` end an idle pane or window at once and
   ask first when the Host says it is busy (the same rule as `Ctrl+W`), not on every press as tmux's
   `confirm-before` does.

## Consequences

- `subak hotkeys check` reports one more `shadows_terminal_input` info (the prefix itself).
- `Ctrl+B` no longer reaches programs by default; `Ctrl+B Ctrl+B` sends it.
- Not yet: custom key tables, `prefix2`, `last-window`/`last-pane`, `bind -n`, describe-key, a
  command prompt (Steps 24–25).

## Alternatives

- **A hard-coded prefix in the window** (Step 16's idea). Rejected: the contract already has
  leaders, rebinding and conflict checks; one engine serves every action.
- **A prefix timeout.** Rejected by the contract (no idle timer); `Esc`, a click or focus loss
  cancel.

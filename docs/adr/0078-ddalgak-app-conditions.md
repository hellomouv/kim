# ADR 0078 — Ddalgak's app and window conditions: evaluated in the hook from a watched cache, unknown never matches

Status: Accepted (Step 77) · Requirements: `DDALGAK-FR-020`, `DDALGAK-FR-021`, `DDALGAK-FR-022`,
`DDALGAK-FR-023`, `DDALGAK-SEC-011` · Related: [DDALGAK_RULE_MODEL §7](../DDALGAK_RULE_MODEL.md),
ADR 0024 (context watchers), ADR 0077 (playground)

## Context

Since Step 65 the rules document has `when` (`apps`, `except_apps`, `window_title`), and every
rule with one stayed inactive (`condition_unsupported`) until the engine could evaluate it.
Step 77 makes the same key do different things in different programs with a deterministic,
explainable precedence, and asks that rules fail safely when the context is restricted or
unknown, with low idle cost and nothing sensitive logged.

## Decision

1. **One matcher, `ddalgak::context::Cond`**, compiled from `when` by the loader, the validator
   and the simulator alike. `exe` compares the file name, `path` the full path (known-folder
   tokens expanded, `/` read as `\`), titles `contains`/`equals`; all case-insensitive. An
   unknown field, matcher or an empty `when` is `condition_unsupported`: the rule stays
   inactive, never unconditional.
2. **Evaluated in the hook, lazily.** A binding with `when` carries a flag; the core checks the
   condition only after the binding's chord matched, through a `Foreground` interface, at most
   once per event (`Probe`). A rule whose condition fails is no candidate (§7.1 step 5), so a
   lower rule, such as a global fallback, takes the key; without one the key reaches the app.
   Keys without a matching conditioned rule ask nothing.
3. **Windows reader.** The foreground watcher (ADR 0024) warms a cache of the foreground
   process path per (window, process) on `EVENT_SYSTEM_FOREGROUND`, outside any hook callback.
   If the foreground changed before the watcher saw it, the hook reads the path then
   (`OpenProcess` with limited query rights and `QueryFullProcessImageNameW`), so a rapid
   focus switch never yields a stale decision. The title is read only for a matched rule with a
   title condition, with `InternalGetWindowText` (no `WM_GETTEXT`, so a hung window cannot stall
   the hook), compared and dropped.
4. **Unknown never matches.** A process at a higher integrity level, one that cannot be opened,
   Ddalgak's own windows, the secure desktop and no window at all give no app: `apps` fail, and
   `except_apps` fail too (Ddalgak cannot tell that the window is not one of them). Fail safe means
   the rule stays quiet and the key works as usual.
5. **Precedence** inside a mode rank: rules with `apps`, then with a title, then priority, then
   file order (§7.1 step 6), by a stable sort under the table's mode rank.
6. **Explain.** `ddalgak explain <keys> --context <json>` runs the Step 76 simulator for a
   context given as the trace's `context` object; traces print a program's file name, never its
   path. The editor's rule form edits `apps` (names or paths), `except_apps` and the title.

## Consequences

- Same key, several programs, one visible answer; the editor, the CLI and the engine agree
  because they share the matcher.
- Titles are never written anywhere (status, activity, logs, traces of the engine); a trace only
  echoes the title the user typed into the explain context.
- Not in this Step: a window-class matcher, time or environment conditions, modifier-presence
  conditions and automatic profile activation per app (opt-in) — tracked in #100.
  Live per-app firing on the real desktop is checked manually; automated tests cannot move the
  user's focus.

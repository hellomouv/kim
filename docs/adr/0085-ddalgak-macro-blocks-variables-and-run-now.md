# ADR 0085: Ddalgak macro blocks, variables, "run now" and the MCP `run` grant

- Status: accepted
- Date: 2026-11-12
- Issue: #101 (follow-up of Step 78, #83, ADR 0079); extends ADR 0082 (agents); requirements
  `DDALGAK-FR-074`, `DDALGAK-SEC-011`

## Context

Step 78 gave macros as ordered steps with a rule-level `times`. AutoHotkey users also need loops
inside a macro, branches on the active window, small counters and text variables, and scripts
want to run a macro without pressing its keys. ADR 0082 said agents can never run a macro.

## Decision

1. **Blocks are markers** (`repeat {times}`, `if {when}`, `else`, `end`) in the flat `steps`
   list, not nested arrays. Every real step stays at the top level, so the consent model
   (permissions, the launch and command fingerprints), redaction, previews and the editor's
   list work unchanged and cannot miss a step hidden in a branch. Nesting is at most 3; the
   loader unrolls repeats and lays out branches as relative jumps (`Branch {skip}`, `Skip(n)`)
   within the existing 1000-stage bound, checked before anything is built.
2. **`if` conditions** reuse a rule's `when` (foreground program and title, evaluated when the
   step runs; unknown never matches) plus `var {name, equals}`.
3. **Variables** (`set_var` with `value` or `add`; `{var:name}` in `type_text` templates): at most
   16 per rule, 1000 characters, for one run only; never stored, logged, shown in the activity,
   previews or `rule show` (lengths only). A variable no step sets is refused when loading.
4. **`ddalgak rule run <id>`** asks the running app (never the CLI itself) to start one active
   run rule as if its keys were pressed; refused while off (`engine_off`) or paused
   (`engine_paused`), for inactive rules and for key-bound rules (`rule_not_runnable`). The rule
   runs with the consent it has; modes and `when` are not asked, typing's focus checks still
   apply.
5. **Agents**: a new MCP grant `run` adds `ddalgak_rule_run`; no other grant includes it. This
   amends ADR 0082's "never run a macro" to "never without the user's `run` grant", and an agent
   still can never grant the consent a rule needs.
6. The simulator reports a winning macro's `unrolled` `{times, steps}`.

## Consequences

- Rules v1 gains five step types and `{var:…}` in templates (additive).
- The editor's macro builder edits markers in place; a nested structure is shown by indentation.
- Not done: expressions, arithmetic beyond `add`, loops until a condition (`while`), or reading
  variables from the clipboard or the screen.

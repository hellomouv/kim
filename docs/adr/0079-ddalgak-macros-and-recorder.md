# ADR 0079 — Ddalgak macros: ordered stages on the worker, a bounded `times`, a recorder that sees only the editor

Status: Accepted (Step 78) · Requirements: `DDALGAK-FR-018`, `DDALGAK-FR-074`,
`DDALGAK-FR-040`, `DDALGAK-SEC-001`, `DDALGAK-SEC-002`, `DDALGAK-SEC-005` · Related:
[DDALGAK_RULE_MODEL §5](../DDALGAK_RULE_MODEL.md), ADR 0034 (typing runs), ADR 0037 (launches),
ADR 0040 (mouse), ADR 0043 (modes), ADR 0077 (playground)

## Context

Until Step 77 each step type ran alone, or together with its own kind (launches, mouse steps), and
`delay` never ran. Step 78 asks for multi-step workflows — keys, text, launches, websites, mouse,
mode and engine steps with waits, a bounded repeat, an error policy and cancellation — built
visually or by an explicit recorder, tested before activation, and manageable headlessly, without
stuck keys, always-on keylogging or damage to the active rules.

## Decision

1. **A macro is a rule** whose steps mix kinds, contain a `delay`, or have `times` above 1. The
   loader compiles each step exactly as it compiles it alone (`compile_stages`) into
   `Action::Sequence(SeqPlan)`. Steps that last while the trigger is held (a remap, a mode hold,
   a held button, a move while held) and `block` stay a rule's only step; a hotstring runs one
   typing step. At most 32 steps, `times` 1–100, at most 1000 stages after unrolling.
2. **Bounded repeat is the rule-level `times`** (additive field of `ddalgak-rules-v1`), unrolled
   when the rules load, so a run can never loop. Nested `repeat`, `if` and user variables are not
   in v1: branching is expressed by rules with conditions (`when`, Step 77) and modes, and
   `type_text` keeps its date/time variables.
3. **The worker runs the stages one at a time**, between its other messages: typing stages as the
   Step 69 runs (every guard, the focus check — kept between typing stages and sampled again after
   a launch, because the opened program may take the focus), waits as timers, mouse stages at once,
   launches on their short-lived thread (polled every 20 ms), mode stages through the hook thread
   (`ModeRequest::Rule` with the stage index), engine stages through the controller. One typing run
   or macro at a time (`busy`); `on_error: stop` ends at the first failing stage, `continue` goes on
   and reports the first failure; the whole run is bounded by 60 s (`timeout`). A pause, stop,
   reload, profile switch or the emergency chord ends a macro where it is (`cancelled`): a typing
   stage stops at its next piece and restores modifiers and clipboard, a launch stops before its
   next target, nothing already done is undone. One activity entry per macro (its mode stages'
   answers are not entries of their own, even when they arrive after the macro ended).
4. **The editor's macro builder**: the effect form edits one step; *Add to macro*, a step list
   with ↑ ↓ *Edit* ✕ (also `Alt+Up/Down`), *Add a wait*, *Repeat* and *If a step fails*.
5. **The recorder** records only keys typed into the editor's own window, after an explicit
   *Record keys*, with a banner the whole time; `Esc` or *Stop recording* ends it. Text becomes
   `type_text`, keys that type nothing and shortcuts become `send_keys`, pauses over 300 ms become
   `delay`; at most 32 steps. Nothing outside the window is seen (no hook, no other program, no
   password field elsewhere, no mouse), and nothing is saved until the user reviews the steps and
   saves the rule.
6. **Headless**: `ddalgak rule add/edit --step <json>... --times <n> --on-error …` write macros;
   `ddalgak test` / `explain` and the playground show every step without running it.

## Consequences

- Macros reuse every existing guard; no new injection path exists.
- A macro cannot run forever or hold a key past its end; cancellation is immediate at the next
  stage boundary.
- Not in this Step: nested repeat, `if`/else and variables, recording the mouse, recording in
  other programs (never planned), and a headless "run this rule now" command — tracked in #101.

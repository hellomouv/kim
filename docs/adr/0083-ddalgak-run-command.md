# ADR 0083: Ddalgak `run_command` steps and the `run-commands` permission

- Status: accepted
- Date: 2026-11-12
- Issue: #102 (follow-up of Step 79, #84); requirements `DDALGAK-SEC-001`, `DDALGAK-SEC-011`

## Context

AutoHotkey scripts commonly run a helper and wait for it (`RunWait`, AHK matrix 4.17). Step 70's
`open_program` starts a program and forgets it; Step 79 deliberately left commands out because a
command is a more powerful thing to grant than opening a program: it runs without a window and
its arguments can tell a general tool (a shell, a script host) to do anything.

## Decision

1. A new step type `run_command` (`program`, `args`, `working_dir`, `timeout_ms`), additive in
   rules v1 (older versions treat it as `step_unsupported`).
2. A new per-rule permission **`run-commands`**, separate from `launch-programs`, whose grant
   carries a fingerprint of the rule's exact `run_command` steps (the same canonical-JSON SHA-256
   as launch consent). Any change to the program, an argument, the folder or the timeout makes the
   rule inactive (`consent_stale`) until approved again. Imports and agents never carry or grant it.
3. Execution: `CreateProcess` of the absolute program with the argument list (Rust's standard
   Windows quoting, one argument per element), `CREATE_NO_WINDOW`, standard streams closed, an
   optional working folder; the worker waits on its own thread (the hook never waits), ends the
   process at the timeout, and records `ok`, `command_failed`, `program_not_found`,
   `launch_failed` or `timeout`. Output is never read, arguments are never logged.
4. Shells are **not** refused (unlike `open_program`'s list): refusing them would only push users
   to wrapper scripts. Instead the consent dialog names the program and every argument, and the
   permission is distinct, so a profile that only opens programs never gains it.
5. Previews (`ddalgak test`, the playground) show the program's file name, whether it exists,
   the argument count and the timeout; nothing is run (the "sandboxed dry run").
6. The debug test host records commands to `commands.jsonl` instead of running them.

## Consequences

- The Kim catalog's permission list gains `run-commands` for Ddalgak (disclosed before install).
- A malicious imported profile still cannot run anything: imported rules are off and without
  consent, and the dialog shows the exact command.
- Not done: capturing output into a variable (would need variables, #101) and running elevated
  (never).

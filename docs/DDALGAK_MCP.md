# Ddalgak for scripts, agents and other Kim tools

Status: **current** (Step 81, [ADR 0082](adr/0082-ddalgak-agents-and-kim.md)) · Requirements:
`DDALGAK-FR-060`, `DDALGAK-FR-062`, `DDALGAK-SEC-006` · Related: [CLI_CONTRACT](CLI_CONTRACT.md),
[INTEROP](INTEROP.md), [DDALGAK_RULE_MODEL §13](DDALGAK_RULE_MODEL.md)

## 1. The headless CLI

Every Ddalgak action has a command with human output and `--json` (the `ddalgak-cli-v1` schema),
documented exit codes, data on stdout and diagnostics on stderr, and `--dry-run` for every change.
No command opens a window; `ddalgak capabilities --json` lists them all.

| Area | Commands |
| --- | --- |
| Rules | `rule list | show | check | add | edit | delete | duplicate | enable | disable | move | revoke`, `rules history | backup | restore | reload | draft …` |
| Modes | `mode list | show | add | edit | rename | copy | delete | enable | disable | set | toggle | reset | status` |
| Profiles | `profile list | show | add | edit | duplicate | delete | apply | save | history | rollback | export | import` (`.ahk` too, Step 80) |
| Macros and actions | `rule add --step '<json>'… --times <n> --on-error … --schedule '<json>'` (Steps 78–79) |
| Test and explain | `test <keys> [context options] [--draft]`, `explain <keys> --context <json>` (Steps 76–77) |
| Engine | `status`, `health`, `engine start|pause|resume|stop`, `enable`, `disable`, `pause`, `resume`, `release`, `quit`, `startup …` |
| Agents and Kim | `mcp serve`, `mcp tools`, `interop invoke` (this Step) |

Two writers at once never lose a change: every write takes the rules file's lock and the revision
(`--if-revision` refuses a stale write with `rule_conflict`); `crates/ddalgak/tests/cli.rs`
(`two_clis_editing_at_once_never_lose_a_change`) runs eight writers in parallel.

## 2. The MCP adapter (opt-in)

`ddalgak mcp serve` speaks MCP (JSON-RPC 2.0 over stdin/stdout, one message per line). It is
started only by your MCP client from a configuration you write; it opens no socket and never
starts Ddalgak. Example for Claude Code (`.mcp.json`):

```json
{ "mcpServers": { "ddalgak": { "command": "ddalgak", "args": ["mcp", "serve"] } } }
```

**Always offered (reading):** `ddalgak_status`, `ddalgak_rule_list`, `ddalgak_rule_show`,
`ddalgak_rule_check`, `ddalgak_mode_list`, `ddalgak_profile_list`, `ddalgak_explain` (which rule a
key would fire in a context; nothing runs). Rule listings carry titles, triggers and step types;
`rule show` replaces typed text and launch targets with counts.

**Changes need your grant**, one area at a time, in the `args`:

| `--allow` | Adds | Note |
| --- | --- | --- |
| `rules` | `ddalgak_rule_enable`, `ddalgak_rule_disable` | a rule that needs consent stays off (`consent_required`) — only you grant consent |
| `modes` | `ddalgak_mode_set`, `ddalgak_mode_reset` | the running engine's mode |
| `profiles` | `ddalgak_profile_apply` | switches the active profile atomically |
| `engine` | `ddalgak_pause`, `ddalgak_resume` | never turns an engine that is off on |

An ungranted tool is not even listed. **No tool can** run a rule or a macro, type or press keys,
open a program or a website, grant consent, write rule content, start the app, read the
clipboard, a window title or typed text.

Every tool is one fixed `ddalgak` command line run without a window, so the adapter can do nothing
the CLI could not. Arguments are checked (IDs, bounded texts; nothing that looks like an option is
passed on — `{"id": "--show-text"}` is refused). Every call is appended to
`%LOCALAPPDATA%\mouv\ddalgak\mcp-audit.jsonl` with its time, tool and outcome only.
`ddalgak mcp tools [--allow …] --json` shows what a configuration offers.

## 3. Kim interop

Installed through Kim (`cargo xtask package ddalgak` lays out the package), Ddalgak provides:

| Capability | Effect | Params → result |
| --- | --- | --- |
| `keys.explain` | read | `{keys, context?}` → the `ddalgak-simulation-v1` trace (no app needed) |
| `rules.list` | read | `{profile?, mode?}` → the rule list |
| `mode.status` | read | — → the modes and the active one |
| `mode.set` | write | `{mode}` → the switch (Kim asks for your consent first: `--allow-write`) |

`kim tools invoke keys.explain --params '{"keys":"Ctrl+Shift+K"}'` works whether or not Ddalgak is
running. Ddalgak also calls other Kim tools through its `tool` step (Step 79): a missing Kim or tool
is `tool_missing`, and nothing is ever installed by Ddalgak or by an interop call.

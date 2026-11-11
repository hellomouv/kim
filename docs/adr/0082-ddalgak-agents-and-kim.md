# ADR 0082 — Ddalgak for agents and Kim: an MCP adapter of fixed CLI command lines, grants per area, an interop provider

Status: Accepted (Step 81) · Requirements: `DDALGAK-FR-060`, `DDALGAK-FR-062`, `DDALGAK-SEC-006`,
`KIM-FR-020`, `KIM-SEC-001` · Related: [DDALGAK_MCP](../DDALGAK_MCP.md), ADR 0076 (Noeul's
adapter), [INTEROP](../INTEROP.md)

## Context

Agents and other Kim tools should inspect and, where the user allows, change Ddalgak without a
GUI, but must never run arbitrary macros, inject input, grant consent or read sensitive content.

## Decision

1. **`ddalgak mcp serve`**: a stdio MCP adapter started only by the user's MCP client. Each tool
   is exactly one fixed `ddalgak` command line, run as this program without a window (`--json
   --no-input`): parity with the CLI by construction, and every CLI guard (consent, locks,
   revision checks, privacy of `rule show`) applies.
2. **Read always, change by grant**: reading tools (status, rules, check, modes, profiles,
   explain) are always offered; `--allow rules|modes|profiles|engine` adds the change tools of one
   area. Nothing runs a rule or macro, types, opens programs, grants consent, writes rule content
   or turns the engine on; agents never grant consent, so enabling a rule that needs consent fails
   with `consent_required`.
3. **Argument validation** before any command line is built: IDs match the rule-ID pattern, texts
   are bounded, nothing starting with `-` is passed on, unknown arguments are refused.
4. **Audit**: `%LOCALAPPDATA%\mouv\ddalgak\mcp-audit.jsonl`, one line per call (time, tool,
   outcome), never arguments.
5. **Kim interop provider**: `ddalgak interop invoke` answers one `kim.interop.request` with the
   same command lines: `keys.explain`, `rules.list`, `mode.status` (read), `mode.set` (write; Kim
   asks the user). New error codes `capability_missing` and `tool_incompatible` (exit 5).
6. **Packaging**: `cargo xtask package ddalgak` (the CLI, the tray app, the editor). The repository
   catalog keeps Ddalgak unreleased until Step 82.

## Consequences

- An agent's reach is exactly a subset of the CLI the user already trusts, chosen per area.
- Live tests, consent and engine-on stay human actions in the GUI or the user's own CLI.

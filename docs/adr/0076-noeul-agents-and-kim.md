# ADR 0076 — Noeul for scripts, agents and Kim

- **Status:** Accepted (Step 62, 2026-11-12)
- **Requirements:** `NOEUL-FR-110`, `NOEUL-FR-111`, `NOEUL-FR-112`, `NOEUL-FR-113`,
  `NOEUL-SEC-001`, `NOEUL-SEC-002`, `NOEUL-SEC-008`, `NOEUL-SEC-010`, `KIM-FR-020`, `KIM-FR-021`
- **Builds on:** [ADR 0070](0070-mcp-server.md) (Subak's MCP server), [ADR 0071](0071-capability-interop.md)
  (interop), [ADR 0023](0023-noeul-app-foundation.md) (the instance and its pipe)

## Decision

1. **The user decides who captures without the keyboard** (`[privacy]`). CLI and IPC captures and
   clipboard reads: `notify` (default, a visible notice), `allow` (silently), `deny`
   (`capture_not_allowed`, before anything is captured or read). Agents (`via: mcp`):
   `mcp_captures = off` by default, always with the notice when on, never the clipboard. The check
   runs in the instance, so no client can skip it; `privacy.cli = allow` does not open the door
   for agents.
2. **Capture to a file** (`capture --output <file.png> [--replace]`, `NOEUL-FR-112`): a PNG without
   metadata at an absolute local path, never over a file unless asked, the path never echoed in
   JSON or logs (`NOEUL-SEC-008`). The clipboard as a target stays planned.
3. **An optional MCP adapter** (`noeul mcp serve`, `NOEUL-FR-113`): stdio only, started only by the
   user's MCP client, never starts Noeul; read tools always, `--allow pin` and `--allow capture`
   for more (two keys for captures: the grant and `privacy.mcp_captures`); an ungranted tool is
   not listed; no clipboard tool; every call audited without paths.
4. **Kim**: Noeul is packaged like Kim and Subak (`cargo xtask package noeul`) so `kim install`
   handles it; it provides `pin.list`, `image.pin` and `screen.capture-file` (interface 1.0.0)
   through `noeul interop invoke` — headless, never starting Noeul; Kim's consent rules for
   `write` apply on top of Noeul's own.
5. **One reference**: [NOEUL_CLI](../NOEUL_CLI.md) sorts every command into headless, instance and
   desktop commands, with the privacy rules, the MCP tools and the Kim contract.

## Alternatives rejected

- **MCP tools that start Noeul** — an agent would put a resident app with global hotkeys on the
  user's machine; the user starts it.
- **A clipboard tool for agents** — the clipboard often holds secrets; pasting stays a user action.
- **Echoing the output path** — paths can name people and projects; the caller knows its path.

## Not done

The clipboard as a capture target (`capture --clipboard`), capture from the CLI with an overlay
selection returned to the caller, Linux. A real agent (Claude Code 2.1.322) was run against the adapter on a
sandboxed synthetic desktop ([NOEUL_CLI §7](../NOEUL_CLI.md#7-verified-with-a-real-agent)). **Not tested:**
an agent against the user's real screen (never done by the development agents).

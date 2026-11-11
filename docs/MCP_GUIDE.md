# Subak MCP guide (for agents and the people who set them up)

- **Status:** current (Step 37, 2026-11-11). Decision: [ADR 0070](adr/0070-mcp-server.md).
  Requirements: `SUBAK-FR-062`, `KIM-FR-021`, `KIM-SEC-013`, `KIM-FR-012`.
- **What it is:** `subak mcp serve`, a [Model Context Protocol](https://modelcontextprotocol.io)
  server over **stdio**. Your agent starts it; there is no network listener, no service and no
  account. MCP is an addition: everything is also in the `subak` CLI (`--json`).

## 1. Set it up

### Claude Code

```powershell
# Read-only (recommended start): list Terminals, read screens, Claude status.
claude mcp add subak -- subak mcp serve

# Let the agent create shells and type into them, and stop/kill with confirmation.
claude mcp add subak -- subak mcp serve --allow create --allow input --allow destructive

# Remove it again.
claude mcp remove subak
```

Or in a project's `.mcp.json` (or `--mcp-config <file>`):

```json
{
  "mcpServers": {
    "subak": { "command": "subak.exe", "args": ["mcp", "serve", "--allow", "create"] }
  }
}
```

Claude Code still asks you before each tool call unless you allow it (`--allowedTools
mcp__subak__list_terminals`, or `/permissions`). Allow read tools freely; keep asking for
`send_text`, `send_keys` and the destructive ones.

### Other agents

Any MCP client that starts a stdio server: command `subak.exe` (or its full path), arguments
`mcp serve` plus the grants you choose. Protocol versions 2025-11-25, 2025-06-18, 2025-03-26 and
2024-11-05; capability `tools` only.

`subak mcp tools [--allow …] --json` prints exactly the tool list a server with those grants
offers, without starting anything.

## 2. Grants (least privilege)

| Grant | Adds | Note |
| --- | --- | --- |
| *(none)* | `list_terminals`, `terminal_info`, `list_panes`, `read_screen`, `wait_for_text`, `claude_status`, `host_stats`, `list_recoverable` | Read only. The Host itself refuses anything else on this connection, and screens are always redacted. Reading never starts a Host. |
| `create` | `create_terminal` (default shell or a profile), `split_pane` | No program of the agent's choice. |
| `run` | `create_terminal` with `command`, `recover_terminal` with `with_commands` | A program of the agent's choice. |
| `input` | `send_text`, `send_keys` | **As strong as `run`:** a shell runs what it is given. |
| `control` | `rename_terminal`, `detach_clients` | Detaching never ends a Terminal. |
| `destructive` | `stop_terminal`, `kill_terminal`, `remove_terminal` | A preview unless `confirm: true`. |
| `recover` | `recover_terminal` | A preview unless `confirm: true`. |

A tool whose grant is missing is not listed; if a client calls it anyway, the result is an error
(`permission_denied`) and nothing runs.

## 3. Tools and their behaviour

| Tool | Arguments | Result | Annotations |
| --- | --- | --- | --- |
| `list_terminals` | — | `{host_running, terminals}` | read only, idempotent |
| `terminal_info` | `target` | `terminal.info` | read only |
| `list_panes` | `target` | windows and panes | read only |
| `read_screen` | `target`, `history_lines` (≤10000), `max_bytes` (1024–262144, default 65536) | `{text, redacted, truncated, terminal_id, pane_id}`; the newest text is kept when cut | read only |
| `wait_for_text` | `target`, `text`, `timeout_s` (≤60, default 10) | `{found, waited_ms}`; cancellable | read only |
| `claude_status` | `target?` | per pane: working, waiting, idle… | read only |
| `host_stats` | — | `{host_running, memory, handles, counters}` | read only |
| `list_recoverable` | — | saved Terminals and what a restore would do | read only |
| `create_terminal` | `name?`, `profile?`, `cwd?`, `command?` (needs `run`) | the new Terminal | not idempotent |
| `split_pane` | `target`, `direction` | the new pane | not idempotent |
| `send_text` | `target`, `text` (≤4096 characters), `enter` (default true) | `{sent_characters}` | destructive |
| `send_keys` | `target`, `keys` (≤64 tmux key names: `Enter`, `C-c`, `Up`…) | `{sent_keys}` | destructive |
| `rename_terminal` | `target`, `name` | old and new name | idempotent |
| `detach_clients` | `target` | detached clients | idempotent |
| `stop_terminal`, `kill_terminal` | `target`, `confirm` | preview (`dry_run: true`, `confirm_needed`) or the result | destructive |
| `remove_terminal` | `target`, `force?`, `confirm` | as above | destructive |
| `recover_terminal` | `target`, `with_commands?`, `claude_resume?`, `confirm` | the plan, or the restored Terminal | destructive |

- **Targets:** a Terminal name or id (`t-…`), a window (`w-…`) or a pane (`p-…`).
- **Errors:** a failed tool returns `isError: true` and `structuredContent.error` `{code,
  message, hint}` with the CLI error codes ([CLI_CONTRACT §6](CLI_CONTRACT.md)), for example
  `target_not_found`, `host_not_running`, `permission_denied`, `invalid_params`, `cancelled`.
  Protocol mistakes are JSON-RPC errors (`-32700` bad JSON, `-32600` bad request or batch,
  `-32601` unknown method, `-32602` unknown tool).
- **Concurrency:** requests are answered in order, one at a time; `notifications/cancelled`
  stops a running `wait_for_text` within ~200 ms. Each Host request has the CLI's time limits.
- **Idempotency:** read tools, rename, detach and the destructive tools on an already-ended
  Terminal can be repeated; create, split and typing cannot.

## 4. Threat model

| Threat | Mitigation |
| --- | --- |
| An agent types or runs something you did not want | No input or program tool without `--allow input`/`run`; your client asks before each call unless you allowed it; destructive tools preview first. |
| Secrets on a screen reach the model | `read_screen` and `wait_for_text` read through the `mcp` connection, for which the Host always redacts (`***`); the CLI's `--no-redact` does not exist here. |
| A remote or other-user process uses the server | stdio only: only the process that started it talks to it. The Host checks the pipe's user and image name both ways (IPC_PROTOCOL §2). |
| A compromised local program of the same user | Out of scope: it could run `subak` itself. The grants protect against the model misusing tools, not against code already running as you. |
| Hidden actions | `%APPDATA%\mouv\subak\mcp-audit.jsonl`: time, tool, target, argument names, confirm, outcome — never typed text, screen text or secrets. Rotated at 1 MiB (`.jsonl.1`). |
| Runaway output | Screens are cut at `max_bytes`; text input at 4096 characters; key lists at 64; waits at 60 s; messages at 1 MiB. |

## 5. Sample workflows (tested in `crates/subak/tests/mcp_server.rs`)

1. **Inspect, read only:** `list_terminals` → `list_panes {target: "work"}` → `read_screen
   {target: "work", history_lines: 200}` → `claude_status`.
2. **Run a build in a new shell** (grants `create`, `input`): `create_terminal {name: "build"}` →
   `send_text {target: "build", text: "cargo build"}` → `wait_for_text {target: "build", text:
   "Finished", timeout_s: 60}` → `read_screen`.
3. **Interrupt and clean up** (grants `input`, `destructive`): `send_keys {target: "build",
   keys: ["C-c"]}` → `kill_terminal {target: "build"}` (preview: show it to the user) →
   `kill_terminal {target: "build", confirm: true}`.

Headless: none of this needs the window. With no Host running, read tools report
`host_running: false`; `create_terminal` starts a Host on demand, as `subak new` does.

## 6. Verified

Windows 10 Pro 10.0.19045, Claude Code 2.1.322: `claude -p … --mcp-config <file>
--strict-mcp-config --allowedTools mcp__subak__list_terminals` started `subak mcp serve`, called
`list_terminals` and answered with the Terminal's name (Step 37). The integration test drives the
same server through every tool group. **Not tested:** other MCP clients.

## 7. Noeul (Step 62)

Noeul has its own adapter, `noeul mcp serve`: read tools always (status, pins, the capture
history), `--allow pin` to pin a local image file, `--allow capture` to capture a region or monitor
to a PNG file — which Noeul additionally refuses until the user runs `noeul config set
privacy.mcp_captures on`, and which always shows a notice. No tool reads the clipboard. Setup,
tools and the audit file: [NOEUL_CLI §4](NOEUL_CLI.md#4-agents-the-mcp-adapter).

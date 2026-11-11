# ADR 0070 — The Subak MCP server: stdio, read-only by default, granted mutations

- **Status:** Accepted (Step 37, 2026-11-11)
- **Requirements:** `SUBAK-FR-062`, `KIM-FR-021`, `KIM-SEC-013`, `KIM-FR-012`
- **Builds on:** [ADR 0019](0019-secure-host-supervisor-and-ipc-v1.md) (client kinds), the `mcp` kind of
  [IPC_PROTOCOL §2](../contracts/IPC_PROTOCOL.md)

## Decision

1. **Transport: stdio only.** `subak mcp serve` is started by the MCP client and speaks JSON-RPC
   2.0, one message per line, on its stdin/stdout (MCP versions 2025-11-25, 2025-06-18,
   2025-03-26, 2024-11-05; the client's version is used when known, else the newest). No
   Streamable HTTP, no port, no service: nothing listens, and only the process that started the
   server can talk to it. Capability `tools` only (no resources, prompts or sampling).
2. **Two connections, two strengths.** Read tools use an IPC connection of kind `mcp`. The Host
   allows that kind only discovery methods and, since Step 37, `pane.capture` — **always
   redacted** for it (the Host overrides `redact`), so a model never sees a secret-looking value
   from a screen. Mutating tools use a `cli` connection named `subak-mcp`, and only exist when
   the user granted them.
3. **Grants are the user's, in the client's server configuration:** `--allow create | input |
   run | control | destructive | recover`. Ungranted tools are not listed and are refused if
   called. `run` (a program of the agent's choice; recovering with saved start commands) is
   separate from `create` (default shell or a profile). `input` is documented as as strong as
   `run`. There is no `all`.
4. **Destructive tools preview** (`dry_run`) unless called with `confirm: true`; their
   `destructiveHint` annotation makes clients ask the user. The server's instructions tell the
   model to show the preview and ask before confirming.
5. **Audit trail:** every call appends `{ts, pid, tool, target, argument_names, confirm, ok,
   error}` to `%APPDATA%\mouv\subak\mcp-audit.jsonl` (rotated at 1 MiB) — never typed text,
   screen text, commands or secrets.
6. **Limits and semantics:** requests are served in order; `notifications/cancelled` stops a
   running `wait_for_text`; screens are cut at 256 KiB (default 64 KiB, newest text kept), typed
   text at 4096 characters, key lists at 64, waits at 60 s, messages at 1 MiB; tool failures are
   tool results with `isError` and the CLI error codes, protocol errors are JSON-RPC errors.
7. **Attach is not an MCP tool**: a live byte stream does not fit request/response; agents read
   with `read_screen`/`wait_for_text` and write with `send_text`/`send_keys`.

## Why not grants in the Host

The Host trusts its own user (ADR 0019); a process of that user could run `subak` directly, so
Host-side grants would add no boundary. The grants protect against the *model* misusing tools;
the `mcp` kind keeps the default server physically read-only even if the server code had a bug.

## Testing

`crates/subak/tests/mcp_server.rs`: the real server and Host — handshake, read tools with no Host
(none started), a token on a screen returned as `***`, ungranted tools absent and refused,
unknown tools and junk JSON, a cancelled wait, create/type/wait/split/keys with grants, `run`
required for a program, a kill previewed then confirmed, input limits, the audit trail without
the typed text. Unit tests: tool lists per grant, annotations, the handshake and refusals.
Claude Code 2.1.322 called `list_terminals` through `claude -p --mcp-config` (Step 37).

# Kim tool interoperability

- **Status:** current (Step 38, 2026-11-11). Decision: [ADR 0071](adr/0071-capability-interop.md);
  contract: [CLI_CONTRACT §9](CLI_CONTRACT.md). Requirements: `KIM-FR-020`, `KIM-FR-021`,
  `KIM-SEC-001`, `KIM-NFR-001`.
- **In one sentence:** a tool says what it *provides* in its capabilities document; another tool
  (or an agent) finds providers by reading files and calls one as a short-lived, headless,
  time-limited process — there is no orchestrator, registry service or daemon, and Kim Manager
  stays an installer, not a runtime.

## 1. Discover

A capability is a stable dotted id with its own SemVer **interface version**, independent of the
tool's version and of its CLI command names:

```json
"provides": [
  { "id": "terminal.open", "version": "1.0.0", "effect": "write",
    "summary": "Open a new Terminal with the default shell or a profile", "network": false, "opens_window": false }
]
```

`kim tools find <capability> [--require-version "^1"] [--probe]` lists the installed providers
with tool, tool version, interface version, effect, absolute executable, `available` (the program
exists) and `compatible` (the interface satisfies the requirement). It reads the manifests Kim's
installer keeps next to each installed version; `--probe` asks each tool (`capabilities --json`)
instead. Nothing is started otherwise.

## 2. Invoke

`kim tools invoke <capability> [--tool <id>] [--require-version <req>] [--params '<json>']
[--allow-write] [--yes] [--allow-window] [--dry-run] [--timeout <s>]` picks the first available,
compatible provider (or `--tool`) and runs

```text
<provider.exe> interop invoke --json --no-input        (stdin: one request, no window)
```

with this request (at most 64 KiB):

```json
{ "schema": "kim.interop.request", "schema_version": 1, "capability": "terminal.open",
  "interface": "1", "params": { "name": "from-noeul" }, "dry_run": false,
  "caller": { "tool": "kim", "version": "0.0.1" } }
```

The provider answers with its normal `--json` envelope; Kim returns `{capability, provider: {tool,
version, interface}, dry_run, result}` where `result` is the provider's `data`. Limits: 30 s by
default (`--timeout` up to 300), 1 MiB of answer, the child killed on expiry.

### Errors

| Situation | Code | Exit |
| --- | --- | --- |
| No installed tool provides it (an old tool without `provides` included) | `capability_missing` | 5 |
| `--tool` names a tool that is not installed, or its program is gone | `not_installed` | 3 |
| The interface does not satisfy the requirement (default `^1`), or the provider refuses the interface major | `tool_incompatible` | 5 |
| The effect needs consent that was not given | `confirmation_required` | 7 |
| The provider did not answer in time | `timeout` | 6 |
| The provider reported an error (its code in `details` as `tool_code: …`, its message redacted), or answered garbage | `tool_failed` | 1 |

**Nothing is ever installed** to satisfy a call; the hints say how to install or update explicitly.

## 3. Consent (CLI_CONTRACT §9.6)

| Effect | Invoked… |
| --- | --- |
| `read` | any time |
| `write` | only with `--allow-write`, which a caller passes only as the direct result of a user action that names the change |
| `destructive` | `--allow-write` **and** `--yes`, after the user confirmed in the caller's own UI |
| `install` | never through interop (`kim install` is explicit) |
| unknown (a newer contract) | as `destructive` |
| `opens_window: true` | never from a headless path; only with `--allow-window` when the user asked for that window |

`--dry-run` asks the provider what it would do; it changes nothing and needs no consent. Providers
check their own limits as well (Subak never starts a program of the caller's choice through
`terminal.open`).

## 4. Capabilities today

| Id | Interface | Provider | Effect | Params → result |
| --- | --- | --- | --- | --- |
| `terminal.list` | 1.0.0 | Subak | read | — → `{host_running, terminals: [{id, name, state}]}`; starts no Host |
| `terminal.open` | 1.0.0 | Subak | write | `{name?, cwd?, profile?}` → `{terminal_id, name, pane_id}`; Host on demand |
| `terminal.paste-path` | 1.0.0 | Subak | write | `{target, path}` (an existing absolute file) → `{typed, pane}`; types the quoted path, no Enter |

Noeul and Ddalgak become providers and callers in their own Steps (Noeul Step 62, Ddalgak Step 81);
until then the workflows below are run by an agent or a script through `kim tools invoke`.

## 5. Workflows

**A screenshot into the Claude Code conversation in a Subak pane** (a user action, so write is
allowed):

```powershell
# A screenshot file the user saved (Noeul's capture-to-file for agents is Noeul Step 62).
kim tools invoke terminal.paste-path --allow-write `
    --params '{"target":"claude","path":"C:\\Users\\me\\Pictures\\capture.png"}' --json
```

**A shortcut that opens a Subak Terminal** (a Ddalgak launch rule):
`kim tools invoke terminal.open --allow-write --params '{"name":"scratch"}' --json`.

**An agent checking first:** `kim tools find terminal.open --require-version "^1" --json`, then
`kim tools invoke terminal.open --dry-run --json` to show the user what would happen, then the
real call with `--allow-write` once the user agreed. No step contacts a network or a cloud service.

## 6. Compatibility and migration policy

- **Additive only within a major.** A provider may add optional params and result fields and bump
  the interface minor (`1.1.0`); callers ignore unknown fields. Removing or changing the meaning of
  anything is a new major (`2.0.0`); a provider may offer both majors as one id while it migrates.
- **Callers state the major they speak** (`interface` in the request, `--require-version` for
  Kim); a provider refuses another major with `tool_incompatible`, never guesses.
- **Old tools** without `provides` are simply not providers (`capability_missing`), and keep
  working through their CLI commands (`kim tools describe --require-command`).
- **The request and envelope formats** are versioned (`schema_version` 1); a v2 is a new contract
  major, announced in CLI_CONTRACT §8.
- **Ids are never reused** for a different meaning.

## 7. Testing

`crates/kim/tests/interop.rs` (with the test-only fixture `crates/kim/examples/interop_fixture.rs`,
copied into a sandbox by the test — never packaged or installed by Kim),
`crates/subak/tests/interop_provider.rs`, `e2e/tests/step38_interop.rs` (release `kim` + `subak`).

# ADR 0071 — Capability-based interoperability without an orchestrator

- **Status:** Accepted (Step 38, 2026-11-11)
- **Requirements:** `KIM-FR-020`, `KIM-FR-021`, `KIM-SEC-001`, `KIM-NFR-001`, `KIM-SEC-013`
- **Builds on:** [ADR 0015](0015-cli-contract-and-tool-discovery.md) (capabilities documents,
  `kim tools describe`, CLI_CONTRACT §9)

## Decision

1. **Capability ids, not command names.** A tool lists what other tools may call under
   `provides` in its capabilities document: a dotted id (`terminal.open`), an interface SemVer
   independent of the tool's version, the effect, network and window facts. The field is
   optional and additive (`tool-capabilities-v1` stays version 1); older documents have none.
2. **One invocation shape:** `<tool> interop invoke --json --no-input` with a
   `kim.interop.request` v1 document on stdin (capability, interface major, params, dry_run,
   caller). JSON on stdin avoids shell quoting and argument injection, and lets every capability
   have typed params without a template language. The answer is the tool's normal envelope.
3. **Kim is the reference caller** (`kim tools find`, `kim tools invoke`), implementing
   CLI_CONTRACT §9.5: resolve from manifests, no shell, no window, stdin closed after the
   request, a time limit (default 30 s), capped output (1 MiB), the provider's errors mapped
   (`capability_missing`, `tool_incompatible` keep their meaning; anything else is the new
   `tool_failed` with the provider's code in `details` and its message redacted by
   `redact::foreign`, which also hides `name=value` secrets and token-shaped words).
4. **Consent per effect** follows §9.6: read freely, write with `--allow-write`, destructive also
   `--yes`, install never through interop, unknown effects as destructive, windows only with
   `--allow-window`; dry runs need no consent. Providers keep their own limits.
5. **No resident process.** Discovery reads files; each call is one short-lived process. Kim
   Manager remains the installer; a tool works without Kim being installed (it can read the same
   manifests or call a known path).
6. **Subak is the first provider** (`terminal.list`, `terminal.open`, `terminal.paste-path`).
   `SUBAK_ENDPOINT` names Subak's default endpoint for a process tree, so a caller's environment
   (a test, a sandbox) reaches the right Host; `--endpoint` still wins.
7. **A test-only fixture** (`crates/kim/examples/interop_fixture.rs`) plays a second tool —
   normal, old, hung, garbage and failing variants. It is an example, never a release binary,
   and tests copy it into their sandbox themselves.

## Consequences

- Each provided capability is a small, documented interface (docs/INTEROP.md §4) that must stay
  additive within its major (§6 there).
- A caller pays one process start per call (tens of milliseconds); idle cost stays zero.

## Testing

`crates/kim/tests/interop.rs`: discovery, read/write/destructive/window consent, dry runs,
interface negotiation, missing capability and tool, a failing provider with a secret in its
message, a broken answer, a hung provider (timeout), an old tool, nothing installed. Unit tests:
ids, consent, redaction. `crates/subak/tests/interop_provider.rs`: Subak's capabilities and
refusals. `e2e/tests/step38_interop.rs`: the release `kim` invoking the release `subak`.

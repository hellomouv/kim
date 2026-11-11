# ADR 0065 — Tabby parity: priorities, placement and the SSH transport criteria

- **Status:** Accepted (Step 32, 2026-11-11)
- **Requirements:** `SUBAK-FR-061`, `KIM-FR-001`, `KIM-FR-020`, `KIM-SEC-013`, `SUBAK-SEC-011`
- **Evidence:** [TABBY_FEATURE_MATRIX.md](../TABBY_FEATURE_MATRIX.md) (209 rows, audited against
  Tabby v1.0.238, still the latest release on 2026-11-11)

## Context

Tabby offers many conveniences through an Electron app and an npm plugin host. Subak must reach
the useful ones without a plugin framework, a background service or an always-on process beyond
its Host, and must not claim parity it has not measured.

## Decision

1. **No plugin framework.** Convenience comes from core data (profiles, snippets, layouts,
   rules) and from small independent Kim tools that call the Subak CLI. Colour schemes and
   translations are data files. npm plugins, the web app, cloud sync and a vault passphrase stay
   excluded (their matrix rows give the reasons).
2. **Placement** (the matrix's "Core vs optional" table is binding): emulation, tabs, panes,
   persistence, search, clipboard, themes, local profiles and SSH sessions are **core** (Host
   owned, so they survive the window); serial and Telnet are **candidate independent bridges**
   that run as pane processes; SFTP *protocol* lives with the SSH session, its browser UI may
   become an independent tool (decided in Step 35).
3. **Priorities for Steps 33–36** are the matrix's checklist: P1 is profiles (33), SSH with
   password and key convenience and Credential Manager secrets (34), file transfer (35); the
   rest follows (36), and Step 36 publishes a parity statement like Step 26's.
4. **SSH transport — criteria for Step 34's choice** between Windows' OpenSSH client
   (`ssh.exe` in a pane's pseudo console) and an in-process Rust SSH library:
   - *password and passphrase convenience* from Credential Manager without echoing the secret
     to a terminal or a file (OpenSSH 8.1 on Windows 10 lacks `SSH_ASKPASS_REQUIRE`);
   - *host-key verification* that is strict by default and visible to the user;
   - *SFTP and forwarding* over the same authenticated connection (Step 35);
   - *supply chain*: the dependency tree passes `cargo deny` and is small enough to audit;
   - *budgets*: an idle session costs no CPU; connecting adds ≤ 50 ms over the network round
     trips; the Host's private memory grows ≤ 2 MiB per session.
   The option that meets them with the smaller attack surface wins; the choice is recorded in an
   ADR before code lands.
5. **Safe defaults** (all Steps): secrets only in Credential Manager; strict host keys; port
   forwards bind to loopback; Telnet off; no global hotkey by default; nothing starts at login
   unless enabled; no network call the user did not ask for.

## Consequences

- Some Tabby conveniences arrive as separate tools or not at all; the matrix says which.
- Each later Step must update its rows with evidence; `cargo xtask docs-check` rejects stale or
  unevidenced rows.

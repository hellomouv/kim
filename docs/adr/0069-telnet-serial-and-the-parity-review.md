# ADR 0069 — Telnet and serial panes, ssh config hosts, profile groups and the parity review

- **Status:** Accepted (Step 36, 2026-11-11)
- **Requirements:** `SUBAK-FR-061`, `KIM-SEC-013`, `KIM-FR-012`, `KIM-NFR-001`
- **Builds on:** [ADR 0065](0065-tabby-parity-priorities-and-ssh-transport-criteria.md),
  [ADR 0067](0067-ssh-through-windows-openssh-and-credential-manager.md),
  [ADR 0068](0068-sftp-forwarding-and-jump-hosts.md)

## Decision

1. **Telnet and serial are panes whose program is `subak.exe` in a bridge mode** (`__telnet
   <host> <port>`, `__serial <port> <settings…>`, accepted only with `SUBAK_BRIDGE=1`, which the
   CLI sets for that pane). The bridge puts its pseudo console in raw VT mode and relays bytes.
   The Host gains no protocol code and never blocks on a socket or a device; a bridge process
   exists only while its pane is open; tabs, panes, detach and GUI close work as for any
   Terminal. When the connection ends, the pane keeps its last screen and a `[subak: …]` line.
2. **Telnet is off by default.** `subak telnet connect` refuses (`invalid_state`, exit 7) unless
   `--allow-insecure` is given, and every connection warns in `data.telnet.warnings`, on stderr
   and in the pane that everything, passwords included, travels unencrypted. Negotiation accepts
   only echo, suppress-go-ahead, binary, terminal type (`XTERM-256COLOR`) and window size (sent
   again on resize); every other option is refused. `IAC` bytes are escaped both ways; Enter is
   CR LF. No Telnet profile type and no Telnet login scripts yet (follow-up #98 T6).
3. **Serial** (`subak serial list`, `subak serial connect COMn [--baud] [--data-bits]
   [--parity] [--stop-bits] [--flow none|rtscts|xonxoff] [--newline cr|lf|crlf]
   [--local-echo]`): ports come from `HKLM\HARDWARE\DEVICEMAP\SERIALCOMM` (read only); the port
   is opened exclusively with `SetCommState`; reads wait at most 50 ms in the kernel, so nothing
   spins and writes are never held long. A missing or busy port is reported in the pane; the
   Host is unaffected. No serial profile type yet (#98 T6). **No serial hardware or virtual COM
   pair is available on the development PC**, so real I/O is not tested.
4. **`subak ssh hosts`** lists the concrete `Host` entries of `~/.ssh/config` (or
   `--ssh-config`) with HostName, User, Port, IdentityFile and ProxyJump, read only; patterns,
   `Match` and `Include` are not followed. `ssh connect <alias>` already uses them through ssh.
5. **Profile groups:** a profile may have `group = "…"` (detected shells are in `Detected`);
   `profile list` shows it. The window's collapsible tree is #98 T4.
6. **The parity review** is [TABBY_PARITY_REPORT](../TABBY_PARITY_REPORT.md): totals by status
   and area, every `implemented` row with its evidence, the excluded and blocked rows with their
   decisions, validation across local, SSH, SFTP, Telnet and serial panes, resource use and the
   safety summary. Every row that targeted Step 36 and is not done names a group (T1–T6) of the
   follow-up issue #98, which lists acceptance criteria per group. **Subak does not claim full
   Tabby parity.**

## Consequences

- Telnet and serial need no new crate and no new Host IPC; the cost is a bridge process per pane
  (~5 MiB working set, no CPU while idle, measured in Step 36).
- A remote pane restored after a reboot re-runs its bridge only with `recover --with-commands`
  (never automatically, `SUBAK-SEC-003`).

## Testing

`crates/subak/tests/telnet_serial.rs` (a real Host, the real bridge, an in-process Telnet server):
refusal without `--allow-insecure` (and no Host started), negotiation (TTYPE, NAWS, ECHO, SGA,
terminal type answer), a login exchange typed through the pane, the server hanging up, the
serial port list and checks, a port that does not exist reported in the pane while the Host keeps
serving, `ssh hosts`. Unit tests: the Telnet state machine (split sequences, refused options,
escaping), serial newline conversion and line settings (`DCB` bits), ssh config parsing, profile
groups. **Not tested:** a real serial device, Telnet servers other than the test server.

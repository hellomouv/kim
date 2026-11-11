# Performance budgets (Step 39)

- **Status:** current (Step 39, 2026-11-12). Requirements: `KIM-NFR-001` (no idle cost),
  `SUBAK-NFR-*` ([PRODUCT_REQUIREMENTS](PRODUCT_REQUIREMENTS.md)).
- **Source of every number:** `cargo xtask bench --record` on the development PC; the file
  [perf/baseline.json](perf/baseline.json) holds the measurements, the machine and the commit.
  Nothing here is estimated.

## 1. How to measure

```powershell
cargo xtask bench            # measure, print against the budgets, write target/bench/latest.json
cargo xtask bench --check    # also fail when a measurement is over its budget
cargo xtask bench --record   # rewrite docs/perf/baseline.json (review the diff in the commit)
```

It builds the release `subak` and `kim`, uses a private endpoint and a sandboxed
`APPDATA`/`LOCALAPPDATA`, opens no window, touches no network, and takes about two minutes. It is
not part of `cargo xtask ci` (timings depend on the machine); run it before a release and after
performance-relevant changes, on an otherwise quiet machine.

## 2. Baseline and budgets

Machine: Windows 10 Pro 10.0.19045, Intel64 Family 6 Model 183 (20 logical processors), release
build at commit `ff5072c` (2026-11-12). A budget is twice the measurement or the measurement plus
a floor (50 ms for times, 2 MiB for memory), whichever is larger; zero stays zero; leaks have
fixed allowances.

| Measurement | Baseline | Budget | What it is |
| --- | --- | --- | --- |
| `host_start_ms_median` / `p95` | 72.2 / 74.5 | 144.4 / 149.0 | `subak host start --no-shell` until it answers (process start, pipe, handshake) |
| `host_idle_private_mib` / `working_set_mib` | 1.2 / 5.5 | 3.2 / 8.3 | a Host owning nothing |
| `first_prompt_ms` | 54.3 | 108.6 | `subak new` returned → cmd's prompt on the screen |
| `new_terminal_ms_median` / `p95` | 23.4 / 27.7 | 73.4 / 77.7 | `subak new <name>` (CLI start + IPC + pane spawn) |
| `host_private_mib_per_terminal` | 0.3 | 2.3 | Host private memory per Terminal (10 Terminals) |
| `host_private_mib_per_pane` | 0.2 | 2.2 | per additional pane (9 splits) |
| `host_working_set_mib_10_terminals` | 8.1 | 12.1 | |
| `cli_capture_ms_median` / `p95` | 16.3 / 17.5 | 66.3 / 67.5 | `subak capture` round trip including the CLI's process start |
| `idle_host_cpu_ms_per_10s` | 0.0 | 50.0 | Host CPU while 10 Terminals and 10 panes sit idle (no timers spin) |
| `idle_state_writes_10s` | 0 | 0 | `state.json` writes while idle |
| `host_sockets` | 0 | 0 | TCP/UDP endpoints of the Host (`netstat -ano`) |
| `output_100k_lines_ms` | 12329 | 24658 | cmd printing 100 000 lines in a pane until the last is on screen (cmd itself is the bottleneck) |
| `output_100k_lines_host_cpu_ms` | 1000 | 2000 | Host CPU for those 100 000 lines (parse, screen, scrollback) |
| `unicode_round_trip` | true | true | Korean, Japanese, Greek, ✓ and an emoji typed and read back |
| `soak_handle_delta` | 50 | 66 | Host handles after 50 create/remove cycles and 200 captures (see §3) |
| `soak_private_mib_delta` | 0.5 | 8.0 | Host private memory after the same soak |
| `mcp_malformed_answered` | true | true | 300 malformed MCP lines each answered, then a valid ping |
| `kim_status_ms_median` | 17.5 | 67.5 | `kim status --json` (reads files only) |

Other recorded figures (Step 36, same PC): a Telnet bridge pane process 5.3 MiB working set, 0 ms
CPU while idle.

## 3. Known limitation: one handle per pane in Windows 10's ConPTY

The inbox pseudo console of Windows 10 (`CreatePseudoConsole`/`ClosePseudoConsole`, build 19045)
keeps one handle — to the exited `conhost.exe` — per pseudo console, even with no program started.
A Host therefore holds one extra handle per pane it ever created (the soak's 50). Subak's own code
adds nothing: `win::conpty::tests::sessions_hold_no_handle_beyond_the_one_the_inbox_conpty_keeps`
and `repeated_sessions_do_not_leak_handles` compare bare pseudo consoles with full sessions and
fail if a session leaks more (known since the ConPTY work, [PTY_PROCESSES §9](PTY_PROCESSES.md);
Step 39 identified the kept handle as the exited conhost's process handle). The handles go
away when the Host exits, which an idle Host does when it owns no Terminal. Impact: a Host that
creates thousands of panes without ever exiting holds thousands of exited process objects
(kernel memory, small each). Not mitigated in code (closing an OS-owned handle would be
guesswork); tracked in [RELEASE_READINESS §3](RELEASE_READINESS.md#3-blockers-and-known-limitations).

## 3a. Claude status motion (Step 44)

The moving Claude marks cost one window timer while a shown mark moves and nothing otherwise.
Measured in `crates/subak/tests/gui_window.rs` (debug build, off-screen window): 10–11 tab-bar
frames in 3 s while a Terminal is in progress, **0.0 ms CPU over 10 s** (below one 15.6 ms
`GetProcessTimes` tick); minimized and static mode: 0 frames, 0.0 ms. The floating badge: 8
frames in 2.5 s, no tray update per frame. Details: [SUBAK_BRAND §5](SUBAK_BRAND.md#5-lifecycle-and-cost).

## 4. Not measured by the bench

- **Attach latency** of an interactive client (`subak attach` needs a console); the attach
  snapshot uses the same data path as `capture`, and `scrollback_replay.rs` checks its content.
- **Window paint and input latency** of `subak-gui` (needs a visible window; manual).
- **Lock/sleep/hibernate** behaviour (#31, maintainer runs `scripts/recovery-matrix.ps1`).
- **Long soaks** beyond the bench's cycles; run the bench repeatedly or raise its cycle counts
  locally.

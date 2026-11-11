# Milestone C — Shutdown, sleep and crash recovery validation

Status: **current — Milestone C NOT passed yet** (Step 31, 2026-11-11). The automated and
crash rows pass; the power transitions (lock, sleep, hibernate, reboot, power loss) need the
maintainer and are **not tested**. Requirements: `SUBAK-FR-040`, `SUBAK-FR-045`,
`SUBAK-SEC-002`, `SUBAK-SEC-003`. Model: [RECOVERY.md](RECOVERY.md), ADR
[0063](adr/0063-durable-terminal-state.md), [0064](adr/0064-cold-recovery-and-claude-resume.md).

**Machine:** Microsoft Windows 10 Pro 10.0.19045 (build 19045), 20 logical processors.

## 1. What each transition should do

| Transition | Processes | What Subak promises |
| --- | --- | --- |
| Client closes or crashes | keep running (the Host owns them) | reattach to the same processes and scrollback |
| Screen lock/unlock, long idle | keep running | the window reconnects; same pids; output continues |
| Sleep / hibernate and resume | keep running (Windows freezes and resumes them) | the same, plus programs may see a clock jump; network connections may drop |
| Host crash | **end** (the pane job objects close with the Host) | the last `state.json` (≤ 500 ms old) is intact; a new Host rebuilds on request |
| Clean shutdown / reboot | **end** | topology saved at the clean stop; cold rebuild; Claude resume on request |
| Power loss | **end** | the last coalesced write survives (atomic, CRC); cold rebuild |

Subak never claims a process survived a reboot, a power loss or a Host crash: rebuilt panes
are new processes with new pane ids.

## 2. Results

| # | Case | How | Result |
| --- | --- | --- | --- |
| 1 | Client crash, detach, window close | `crates/subak/tests/milestone_a.rs::a_detached_host_keeps_shells_a_dev_server_and_an_interactive_program_through_every_client_event` (Milestone A) | **PASS** (same pids, dev server answered) |
| 2 | Host crash with cmd, PowerShell, a job and a multi-window Terminal | `crates/subak/tests/milestone_c.rs::a_crashed_host_leaves_an_intact_state_and_a_cold_rebuild_starts_nothing_twice` | **PASS**: all 5 panes ended with the Host; `state check` valid; cold rebuild of 3 Terminals / 5 panes in **99 ms**; same Terminal id and topology; new pids; the job not re-run; a second recover refused |
| 3 | Host crash via the matrix script | `scripts/recovery-matrix.ps1 prepare`, `Stop-Process` of the Host, `check-cold` | **PASS**: 4 Terminals recoverable, rebuilt in 79 ms, same id/windows/panes, new pids, duplicate refused |
| 4 | Live check without a transition (script sanity) | `prepare`, `check-live` | **PASS**: same Host and pane pids, the job ticking, health 11 ms |
| 5 | Interrupted save, torn file, newer version, rollback | `crates/subak/tests/state_persistence.rs` (Step 29) | **PASS** |
| 6 | Opt-out: `never`, `forget`, nothing restored without `--yes`, `auto` only for marked Terminals | `state_persistence.rs`, `recovery.rs` (Steps 29–30) | **PASS** |
| 7 | Claude conversation identity across a Host restart | a real Claude Code 2.1.322 conversation, `subak recover --claude-resume` (Step 30 report) | **PASS** (Host stop/start, not a reboot) |
| 8 | Repeated resume, duplicate Claude start | `recovery.rs::claude_sessions_resume_once_only_when_found_and_asked` | **PASS** |
| 9 | Stale status dots after a lost hook | `claude_status.rs` (Step 28) | **PASS** |
| 10 | Screen lock / unlock | `recovery-matrix.ps1 prepare` → lock → unlock → `check-live -Transition lock` | **NOT TESTED** (needs the maintainer at the PC) |
| 11 | Long idle (hours) | `prepare` → idle ≥ 2 h → `check-live -Transition idle` | **NOT TESTED** (Milestone A measured 15 s idle: 0 ms CPU) |
| 12 | Memory pressure / paging | `prepare` → a memory-heavy workload → `check-live -Transition memory` | **NOT TESTED** |
| 13 | Sleep / resume | `prepare` → Start ▸ Sleep → resume → `check-live -Transition sleep` | **NOT TESTED** |
| 14 | Hibernate / resume | `prepare` → `shutdown /h` → resume → `check-live -Transition hibernate` | **NOT TESTED** |
| 15 | Clean reboot + topology + Claude resume | `prepare` (and a Claude session in a pane with the hooks) → reboot → `check-cold -Transition reboot` → `subak recover <t> --claude-resume --yes` | **NOT TESTED** |
| 16 | Forced power interruption | `prepare` → cut power (or a VM reset) → `check-cold -Transition power` | **NOT TESTED** |

## 3. How to run the open rows (maintainer)

```powershell
cargo build --release -p subak
.\scripts\recovery-matrix.ps1 prepare            # its own endpoint and state; never yours
# do the transition, then:
.\scripts\recovery-matrix.ps1 check-live -Transition sleep     # rows 10–14
.\scripts\recovery-matrix.ps1 check-cold -Transition reboot    # rows 15–16
.\scripts\recovery-matrix.ps1 cleanup
```

Each phase writes JSON evidence to `%LOCALAPPDATA%\mouv\subak\matrix-evidence\<time>\` (pids,
topology, timings, verdict; no secrets). Attach the files to #31 and update the table.

## 4. Defects found and fixed during Steps 29–31

- A Host waiting for a stale Claude deadline held its state lock (Step 28, fixed before push).
- The floating-manager leak test measured its baseline too early (fixed in Step 27).
- The Step 03 state fixture carried a placeholder CRC (fixed in Step 29).

## 5. Acceptance conclusions

- *Real Windows tests demonstrate lock/sleep reconnect and reboot topology + Claude resume:*
  **not yet** — rows 10–16 are open; crash rows and Host-restart resume pass.
- *Recovery never falsely promises continuous execution across shutdown; failure and consent
  paths documented:* **met** (§1, RECOVERY.md, ADR 0064; rebuilt panes are new processes).
- *Milestone C passes only with the required evidence:* **Milestone C is not passed**; #31 stays
  open with rows 10–16 as its blockers.

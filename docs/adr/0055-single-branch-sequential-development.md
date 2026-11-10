# ADR 0055 — Single-session, single-branch, sequential Step development

Status: **Accepted** · Date: 2026-11-11 · Maintainer decision · Supersedes the parallel
master/worker practice (per-Step branches and worktrees, up to three worker agents) used for
Steps 07–75 · Keeps [AGENTS.md](../../AGENTS.md) §2–§5 unchanged

## Context

Steps 07–75 were developed by a coordinating agent and up to three worker agents, each on its own
`step-NN-*` branch in its own Git worktree with its own Cargo target directory, merged into `main`
with `git merge --no-ff`. This raised throughput but duplicated context across agents (token
cost), multiplied build output on disk (four target directories reached about 69 GB, one worker
directory alone 31 GB), and required merge gates, re-runs after every merge and a watchdog. The
maintainer paused it on 2026-11-04 as too costly for its pace, and on 2026-11-11 asked for one
developer working through the remaining Step issues in order.

## Decision

1. **One session, one folder, one branch, one Step at a time.** Development happens in the single
   canonical checkout on `main`. No per-issue branches, no extra worktrees, no parallel coding
   agents or subagents, no separate repositories per tool.
2. **Order.** The next Step is the earliest unfinished Step by the number in the issue title whose
   dependencies are met. A blocked Step is recorded with its exact blocker (in its issue and in
   `DEV_CHECKPOINT.md`) and never closed; the next independent Step is taken meanwhile.
3. **The gate stays local.** Before a Step's commits are pushed, `cargo xtask ci` passes on `main`
   (and `cargo deny` when dependencies change). There is still no CI on push and no pull request;
   the `Daily` workflow still runs the e2e suite once a day.
4. **Unchanged:** product rules, tests and an e2e scenario per feature, the ledger, docs moving with
   code, requirement traceability, commit-and-push per completed item, the Korean completion
   report per issue, closing only on genuinely passing acceptance criteria, Windows only on this PC.
5. **Continuity.** `DEV_CHECKPOINT.md` at the repository root records the current and last
   completed Step, the commit, blockers, verification done and the next action, so a new session
   can resume.

## Consequences

- One Cargo target directory instead of four; no merge commits between Steps (history is linear on
  `main`; earlier `--no-ff` merge commits stay).
- Lower throughput, lower token and disk cost, no merge conflicts between workers.
- The consolidation on 2026-11-11 found every Step branch already merged into `main` (no unique
  commits, no uncommitted work, no open pull requests); the merged branches, the empty worktree
  folder and the worker target directory were removed.

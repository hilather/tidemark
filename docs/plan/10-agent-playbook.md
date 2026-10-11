# 10 — Agent playbook: how to run the waves

This plan is written to be executed by **coding agents working in parallel waves**, with a human (or a lead agent) coordinating. It works with Claude Code subagents, separate Claude Code sessions, or any comparable agent runner. Nothing here depends on one specific tool.

## 1. Roles

| Role | Who | Responsibilities |
|------|-----|------------------|
| **Coordinator** | A human, or a lead agent session supervised by a human | Starts each wave; launches one implementer per task; tracks status; triages spec-change requests; merges; runs gates |
| **Implementer** | One agent per task | Implements exactly one task in its own branch/worktree; owns only the paths listed in the task; writes tests and a handoff note |
| **Reviewer** | A fresh agent that has **not** seen the implementation session | Reviews each task branch against the task spec, the acceptance criteria and AGENTS.md; runs the tests; produces a findings list |
| **Integrator** | Usually the coordinator | Merges task branches in the prescribed order into `integration/wave-N`; resolves trivial conflicts; runs the wave gate |
| **Human gatekeeper** | Engineer with VM/staging access | Runs privileged and performance suites; approves the gate; owns production steps |

The reviewer must be independent so the work isn't grading itself.

## 2. Repository setup (once, before Wave 0)

```bash
git init tidemark && cd tidemark
mkdir -p docs/plan docs/handoff
cp -r <this-plan>/* docs/plan/            # this folder
cp docs/plan/agents/AGENTS.md ./AGENTS.md  # agent conventions at repo root
git add . && git commit -m "Add Tidemark plan"
```

Recommended: a Rocky 8 VM reachable over SSH for `xtask vm-test`, and a staging host for performance work. Agents without VM access write privileged tests but mark them "pending VM run" in their handoff note.

## 3. Branches and worktrees

- `main`: only gated waves land here. Tag `wave-N-done` after each gate.
- `integration/wave-N`: created from `main` at wave start; task branches merge here.
- `wN/<task-id>-<slug>`: one per task, for example `w1/W1-B-walker`. Each implementer works in its **own git worktree** so parallel agents never share a working directory.

```bash
git worktree add ../tm-W1-B -b w1/W1-B-walker integration/wave-1
```

## 4. File ownership

Every task lists **owned paths**. Rules:

1. An implementer edits only its owned paths. It may *read* anything.
2. Wave 0 creates every crate, every module file and every CLI subcommand file as compiling stubs. Later tasks only fill in files they own, so they almost never conflict.
3. The shared files `Cargo.toml` (workspace), `crates/*/Cargo.toml` and `crates/*/src/lib.rs` may be edited **only to add dependencies or `mod` lines**, and each such change is listed in the handoff note. The integrator resolves those trivial conflicts.
4. `tidemark-core` is **frozen** after Wave 0. Changes need a spec-change request (§6).

## 5. Task lifecycle

```
ready → in-progress → implemented → in-review → (changes-requested → in-progress)* → approved → merged
```

1. **Launch:** the coordinator gives the implementer the prompt template (`agents/prompt-templates.md`) filled in with the task ID.
2. **Implement:** the agent reads `AGENTS.md`, the task section, and the docs the task lists under "Read first". It implements, tests, and writes `docs/handoff/<task-id>.md` (template in `agents/handoff-template.md`).
3. **Self-check (Definition of Done, below):** all items must be ticked in the handoff note.
4. **Review:** a fresh reviewer agent gets the reviewer prompt, the task spec, and the branch diff. It produces findings classed **blocker / should-fix / nit**. Blockers go back to the implementer.
5. **Merge:** the integrator merges approved branches into `integration/wave-N` in the order the wave file gives.

### Definition of Done (every task)

- [ ] Only owned paths changed (plus allowed shared-file edits, listed in the handoff note)
- [ ] `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean
- [ ] `cargo test --workspace` passes without privileges
- [ ] Privileged tests written, compiled only with `--features privileged`, and listed in the handoff note with status (`passed on VM <host>` or `pending VM`)
- [ ] Public items have rustdoc; every `unsafe` block (only in `tidemark-sys`) has a `// SAFETY:` comment
- [ ] All acceptance criteria in the task are met, each with the evidence (test name or command output) in the handoff note
- [ ] No TODOs without an issue reference; no `unwrap()` / `expect()` outside tests and provably infallible spots (commented)
- [ ] Handoff note written: what was built, deviations from spec, known gaps, how to test, follow-ups

## 6. When the spec is wrong or incomplete

Agents will find gaps; that's expected. The rule is **don't silently diverge**:

1. If the gap is local to the task (an internal design detail), decide, implement, and record the decision in the handoff note under "Deviations".
2. If it affects a contract (`tidemark-core` types, snapshot schema, CLI flags, config keys, metric names, API), stop and write a **spec-change request** (`docs/handoff/SCR-<n>.md`): the problem, the proposed change, and the affected tasks. Continue with work that doesn't depend on it.
3. The coordinator decides, updates `docs/plan/*`, and broadcasts the change to running agents. Contract changes mid-wave should be rare; batch them for the next wave when possible.

## 7. Wave gates

At the end of each wave, the integrator runs:

1. Merge all approved task branches into `integration/wave-N`.
2. `cargo xtask ci`: fmt, clippy, tests, deny.
3. `cargo xtask vm-test` on Rocky 8.8 and 8.10 (human gatekeeper).
4. The wave-specific gate checklist in the wave file.
5. A **gate review agent** (fresh context) checks cross-task consistency: duplicated helpers, inconsistent naming, contract drift. Findings are fixed in follow-up tasks.
6. The human gatekeeper approves; merge to `main`; tag `wave-N-done`.

## 8. Running a wave with Claude Code (one way to do it)

In a lead session at the repo root, ask something like:

> Run Wave 1 as described in `docs/plan/waves/wave-1-primitives.md`. For each task W1-A … W1-E, launch a separate implementer subagent working in its own git worktree on branch `w1/<task-id>-<slug>` from `integration/wave-1`, using the implementer prompt in `docs/plan/agents/prompt-templates.md`. When an implementer finishes, launch a fresh reviewer subagent for that branch with the reviewer prompt. Report status as a table. Don't merge anything until I approve.

Then, for the gate:

> All Wave 1 tasks are approved. Merge them into `integration/wave-1` in the order the wave file gives, run `cargo xtask ci`, run the gate review with the gate prompt, and give me the checklist results and the exact `xtask vm-test` command to run on the VM.

Keep the human in the loop for merges to `main`, VM/staging runs, and anything touching production.

## 9. Parallelism and sequencing

```
W0 ──▶ W1 ──┬──▶ W2 ──┬──▶ W3 ──┐
            │         └──▶ W4 ──┤
            └──────────▶ W5 ────┴──▶ W6 ──▶ W7
```

| Wave | Parallel tasks | Can overlap with |
|------|----------------|------------------|
| W0 Foundation | 1 agent (three sequential parts) | — |
| W1 Primitives | 5 | — |
| W2 Scan engine | 3 | W5 (after the W1 gate) |
| W3 Queries and reports | 3 | W4, W5 |
| W4 LVM and staged deploy | 2 | W3, W5 |
| W5 Live monitoring | 4 | W2, W3, W4 |
| W6 Daemon and packaging | 3 | — (needs W3, W4, W5) |
| W7 TUI, performance, hardening, pilot | 4 | — |

At most about 8 agents run concurrently at peak (W3 + W4 + W5 overlapping is 9 tasks; cap it to what you can review). **Review capacity, not agent capacity, is the real bottleneck.** Don't start more tasks than you can review within a day.

## 10. Rough effort

Agent wall-clock time is hard to predict and depends on review cadence. As planning guidance:

- W0–W2 take a few days each, including gates.
- W3–W5 overlapping take about a week.
- W6 takes a few days.
- W7 depends on staging availability and the pilot change window.

Gates involving VM and staging runs are usually the critical path, so book that time early.

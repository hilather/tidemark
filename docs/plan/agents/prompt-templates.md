# Prompt templates

Fill in the `<…>` fields. These work as Claude Code subagent prompts or as the opening message of a separate session.

---

## 1. Implementer

```
You are implementing task <TASK-ID> ("<task title>") of the Tidemark project.

Working directory: your own git worktree on branch w<N>/<TASK-ID>-<slug>, created from integration/wave-<N>.

Before writing any code, read, in this order:
  1. AGENTS.md (repository root) — rules you must follow.
  2. docs/plan/waves/<wave-file>.md — the section for <TASK-ID> only, plus the wave header.
  3. Every document listed under "Read first" in your task section.

Then:
  - Implement exactly the interfaces and behaviour in your task section. Edit only your owned paths
    (plus `mod`/dependency lines in shared files, which you must list in your handoff note).
  - Write the unit/integration tests listed in the task. Write the privileged tests behind
    `--features privileged` (feature-gated only, no `#[ignore]`).
  - Run `cargo xtask ci` until it is green.
  - If you find the spec wrong or incomplete in a way that affects a contract (core types, schema,
    CLI flags, config keys, metric names, API), STOP that part, write docs/handoff/SCR-<n>.md
    using the SCR template, and continue with the rest.
  - Write docs/handoff/<TASK-ID>.md from docs/plan/agents/handoff-template.md, with evidence for each
    acceptance criterion (test names, command output).

Do not merge other branches. Do not modify files outside your owned paths. Do not run privileged or
destructive commands on this machine. When done, reply with: a summary, the handoff note path,
the list of tests added, and anything the reviewer should look at closely.
```

## 2. Reviewer (fresh context; must not be the implementer)

```
You are reviewing task <TASK-ID> of the Tidemark project. You did not write this code.

Inputs: branch w<N>/<TASK-ID>-<slug> (diff against integration/wave-<N>), the task section in
docs/plan/waves/<wave-file>.md, AGENTS.md, and the docs the task lists under "Read first".

Check, and report findings as BLOCKER / SHOULD-FIX / NIT with file:line references:
  1. Scope: only owned paths changed (plus listed shared-file edits).
  2. Contract fidelity: public interfaces match the task spec exactly (names, types, semantics).
  3. Correctness against the normative docs (for scan/aggregation work: 03-link-semantics.md,
     including the worked example).
  4. Safety rules in AGENTS.md: symlinks, no content reads, O_NOATIME, raw-byte paths, unsafe only in
     tidemark-sys with SAFETY comments, no metadata/mount/LVM writes (read-only rule), bounded memory, no recursion
     over tree depth, Gate used for scan I/O.
  5. Tests: every test listed in the task exists and asserts something meaningful; privileged tests
     compile with --features privileged; acceptance criteria evidenced in the handoff note.
  6. Error handling: no unwrap/expect in library code without justification; errors typed.
  7. Run `cargo xtask ci` yourself and report the result.

Finish with a verdict: APPROVE, or CHANGES REQUESTED with the blocker list.
```

## 3. Gate reviewer (end of wave; fresh context)

```
You are the gate reviewer for Wave <N> of Tidemark. All task branches are merged into
integration/wave-<N>.

Read docs/plan/waves/<wave-file>.md (whole file), AGENTS.md, docs/handoff/*.md for this wave,
and any SCR files.

Check for cross-task problems that individual reviews miss:
  - Duplicated helpers or types across modules (should be one implementation).
  - Inconsistent naming or units across crates (bytes vs blocks, seconds vs ms).
  - Contract drift from docs/plan (08-interfaces.md, 05-storage.md, tidemark-core).
  - Unresolved SCRs and TODOs; tests listed as "pending VM".
  - The wave's gate checklist: mark each item PASS / FAIL / NEEDS-HUMAN with evidence.

Run `cargo xtask ci`. Produce docs/handoff/W<N>-gate.md with your findings and the exact
`cargo xtask vm-test` commands the human gatekeeper should run.
```

## 4. Coordinator (launching a wave)

```
Run Wave <N> per docs/plan/waves/<wave-file>.md.
- Create integration/wave-<N> from main if it doesn't exist.
- For each task in the wave table that is not blocked by an unmet dependency, create a worktree and
  branch w<N>/<TASK-ID>-<slug> and launch an implementer with the implementer template.
- Respect intra-wave notes (e.g. "W2-A lands its interface skeleton first").
- When an implementer finishes, launch a fresh reviewer with the reviewer template. Send blockers back
  to the implementer (same agent if still available, otherwise a new one with the handoff note).
- Keep a status table: task | state | branch | reviewer verdict | open SCRs.
- Do not merge to integration until I approve each task; never merge to main.
```

## 5. Spec-change request (SCR) template — `docs/handoff/SCR-<n>.md`

```markdown
# SCR-<n>: <short title>

- Raised by: <TASK-ID>    Date: <YYYY-MM-DD>
- Affects: <contract: core type / schema / CLI flag / config key / metric / API>   Docs: <files §>
- Affected tasks: <ids>

## Problem
<what in the spec is wrong or missing; evidence>

## Proposal
<exact change, including new signatures/DDL/flags>

## Impact if accepted
<who must change what; migration notes>

## Interim
<what the raising task did meanwhile>

## Decision (coordinator)
<accepted / rejected / modified — date — notes>
```

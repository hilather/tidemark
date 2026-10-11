# Handoff — <TASK-ID>: <task title>

- **Branch:** w<N>/<TASK-ID>-<slug>  **Agent/session:** <id>  **Date:** <YYYY-MM-DD>
- **Status:** implemented | blocked (see below)

## What was built
<2–6 sentences: modules, main types and functions, notable design decisions>

## Shared-file edits
| File | Change | Why |
|------|--------|-----|
| crates/<crate>/Cargo.toml | added `<dep>` | <reason> |

## Deviations from the spec
| Spec reference | What I did instead | Why | SCR? |
|----------------|--------------------|-----|------|

## Acceptance criteria evidence
| Criterion (copied from task) | Evidence (test name / command + output excerpt) | Status |
|------------------------------|-------------------------------------------------|--------|

## Tests
- Unprivileged: <list>
- Privileged: <list> — status: passed on <VM host + kernel> | pending VM

## Measurements (if the task asks for any)
<numbers, hardware, command used>

## Known gaps and follow-ups
- <gap> — suggested follow-up task

## Notes for the reviewer
<anything subtle: unsafe blocks, concurrency, tricky edge cases>

## Definition of Done
- [ ] Only owned paths changed (plus the shared-file edits listed above)
- [ ] `cargo xtask ci` green
- [ ] Privileged tests written and listed with status
- [ ] Public items documented; `//! Owned by:` headers present
- [ ] All acceptance criteria evidenced above
- [ ] No unexplained TODOs, `unwrap`s or `expect`s

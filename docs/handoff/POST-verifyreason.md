# POST-verifyreason — deploy verify includes current core reasons

## Needs from coordinator

Independent review, uid 1000 / umask 022 execution with sockets, and GitHub ubuntu-24.04 remain pending. Privileged execution remains pending VM; tests are compiled only. This handoff is an evidence candidate, not release approval.

## What changed

For each unique non-ok core component (sampler, scheduler, api, tswriter), the current summary appends its live detail in parentheses. Healthy entries remain `name=ok`; missing and duplicate entries retain their previous state-only wording. The healthy predicate, grace, probe floor, historical labels, flags, exit codes, JSON fields and schemas are unchanged.

The private reason formatter uses the human renderer's `diagnostic` escaping (for example newline becomes `\x0A`). It bounds escaped reasons to 200 UTF-8 bytes including `…`, preserving whole Unicode scalars and escaped control tokens. It stops reading the detail once the bound is exceeded, avoiding allocations proportional to an oversized detail. No shared files, dependencies, manifests, public APIs or unsafe code changed.

## Regression evidence and output

`verify_core_reasons_in_human_and_json_detail` fails against the original production module: exit 101, 0 passed / 1 failed, displaying `sampler=degraded; scheduler=ok; api=ok; tswriter=ok`. The first build overlapped implementation edits, so its passing result was discarded as baseline evidence; the retained `library/before.log` is a subsequent run with the original module restored, followed by restoration of the fix.

The fixed test checks all four core positions with a fake API and clock, including `not mounted: /mnt/x`, newline/ESC/NUL, multibyte overlong text, exactly 200 ASCII bytes, and 201 ASCII bytes. It checks the human renderer's shared diagnostic transformation and serialized JSON `items[].detail`, unhealthy status, and suppression of healthy-component details. The focused deploy suite passes 61 tests.

Before: `FAIL core_components    sampler=degraded; scheduler=ok; api=ok; tswriter=ok`

After: `FAIL core_components    sampler=degraded (not mounted: /mnt/x); scheduler=ok; api=ok; tswriter=ok`

JSON retains the same fields, with `detail` now `sampler=degraded (not mounted: /mnt/x); scheduler=ok; api=ok; tswriter=ok`.

Updated pinned expectation: the shared `verify_probe_floor_keeps_fresh_checks` helper used by `verify_probe_floor_keeps_fresh_service_error` and `verify_probe_floor_keeps_fresh_config_mismatch` now expects the historical core summary to include `(measurement pass in progress)`. The historical prefix and timed last-status wording are unchanged; this is the previous observation's richer summary.

## Assumptions

- The explicit POST task supersedes generic wave selection and branch creation. Read W4-B and its deployment contracts, playbook, prior POST-verify3/4 handoffs, W0-A cache guidance and handoff template.
- The 200-byte bound applies to the appended escaped reason, excluding the component/state label and parentheses.
- A unique non-ok component with an empty detail renders `name=state ()`; duplicate entries remain state-only as requested.
- No new privileged scenario is needed for this formatting change. Existing privileged tests compile; VM execution is pending.
- Validation uses host Rust/Cargo caches offline, uid 0 and umask 022. Socket-dependent coverage on other runners remains a coordinator gate.
- No branches, merges, pushes, service actions, host configuration changes or out-of-scope repository edits.

## Validation

Detailed logs are in the attempt output `library/`. All long commands use `timeout 1500`; Cargo uses offline mode. 

- `cargo fmt --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- `cargo test --offline --workspace --no-fail-fast`: passed; 1,016 passed, 0 failed, 14 ignored across 87 summaries (including child harnesses).
- `cargo build --offline --workspace --features privileged --tests`: passed; execution pending VM.
- `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`: passed.
- `tools/ci.sh` with `CARGO_NET_OFFLINE=true`: passed; 1,018 passed, 0 failed, 14 ignored across 88 summaries (including strict-ancestry/child harnesses). Source checks (150 owner tags), formatting, Clippy, strict-ancestry, workspace tests and privileged-feature build passed. `cargo-deny` is absent, so dependency-policy validation was skipped.
- Focused deploy suite: 61 passed, 0 failed. Original-module negative control: 0 passed, 1 failed, expected exit 101.
- `git diff --check`: passed. Only owned paths changed; no shared-file edits or spec deviations. Commit, build cleanup, clean status and submission are recorded in the attempt report.

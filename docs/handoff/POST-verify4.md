# POST-verify4 — preserve fresh checks when the API probe floor applies

## Needs from coordinator

Independent review, uid 1000 / umask 022 execution with sockets, and GitHub ubuntu-24.04 remain pending. Privileged execution remains pending VM; feature-enabled tests are built only. This handoff is an evidence candidate, not release approval.

## What changed

The post-service/file probe floor now keeps the checks collected by the current poll instead of returning the previous result. It records an API failure without issuing another request, and the existing unavailable-health path fails the core check. Previous API and core observations appear only in explicitly historical detail; the existing monotonic sampler-history logic supplies its age. Fresh service errors, stopped-service hints and config evidence therefore survive.

No shared files, dependencies, manifests, public interfaces, healthy predicate, exit codes, flags, JSON fields, schemas, floor, defaults or transport deadlines changed. Only owned deploy source/tests and this handoff changed.

## Regression evidence and wording

`verify_probe_floor_keeps_fresh_service_error` and `verify_probe_floor_keeps_fresh_config_mismatch` both failed before the fix (exit 101: 0 passed, 2 failed). The original implementation returned `service Pass: ok` and `config_hash Pass: report matches staged config`, respectively, despite the new failures. See attempt artifact `library/before.log`.

Both fake-clock cases first observe routine sampler progress, sleep 500 simulated ms, then consume three simulated seconds in the next service probe. They assert two service calls, one API call, unhealthy output, the fresh failing check and diagnostic, failed current API/core checks and historical labels. The config case changes the report after the first API observation, so the next poll reads a fresh mismatch. No real sleep, socket, service action or millisecond wall-clock assertion is used by these tests.

The skipped API detail is `API request skipped: remaining allowance below 250 ms; historical API observation: Pass (ok)`. Core detail starts with the existing `current daemon health unavailable` explanation, adds `historical core observation: Fail (sampler=degraded; scheduler=ok; api=ok; tswriter=ok)`, and ends with `last status 3500 ms ago: sampler=degraded (measurement pass in progress)`. Historical passes are text within failed current checks, never current passing check items.

## Assumptions

- The explicit POST task supersedes generic wave selection and branch creation. Read the full review, prior handoff, W4-B and its deployment contracts, playbook, W0-A cache guidance and handoff template.
- Keep the initial short-budget probe exception and the other floor locations unchanged; this task fixes the post-probe loss of evidence.
- Retain the complete previous core summary as historical context as well as the timed sampler observation. Successful service/config checks are current only when freshly collected.
- Existing privileged tests cover deployment; no new privileged scenario is needed for this deterministic policy change. VM execution is pending.
- Use host Rust 1.98.0 and Cargo cache offline, uid 0, umask 022. Socket-dependent tests may return early under this sandbox; other runners remain coordinator gates.
- Synchronous filesystem/service work can still overrun the polling allowance, as documented previously.

## Validation

Detailed logs are in the attempt output `library/`. Every long command uses `timeout 1500`; Cargo uses the host cache offline.

- `cargo fmt --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- `cargo test --offline --workspace --no-fail-fast`: passed; 1,010 passed, 0 failed, 14 ignored across 87 summaries (including child harnesses). CLI library: 152 passed, 1 ignored.
- `cargo build --offline --workspace --features privileged --tests`: passed; execution pending VM.
- `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`: passed.
- Focused deploy suite: 65 passed, 0 failed. Final-source regressions: 2 passed, 0 failed; `library/after.log`.
- `tools/ci.sh` with `CARGO_NET_OFFLINE=true`: passed; 1,012 passed, 0 failed, 14 ignored across 88 summaries (including strict-ancestry/child harnesses). Source checks (150 owner tags), format, Clippy, strict-ancestry, workspace tests and privileged-feature build passed. `cargo-deny` is absent, so dependency-policy validation was skipped.
- `git diff --check`: passed. No shared-file edits or spec deviations. No new public items, unsafe code, TODOs or production unwraps.

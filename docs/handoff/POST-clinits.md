# Handoff — POST-clinits

## Needs from coordinator

- **Item 1 remains blocked by the explicit path scope.** Allow a change to `crates/tidemark-cli/src/cmd/mod.rs` (not owned by this task), then apply/review `library/proposed-context-fix.patch` from this attempt output and restore/run `library/blocked-context-test.rs` in the CLI unit module. The outer prefix is chosen by `cmd::error_context` in that file, before the owned command functions run. Changing an inner error cannot replace that already-selected prefix. No out-of-scope repository file was edited. This candidate does **not** claim all four acceptance items are complete.
- Run the socket-capable uid 1000 / umask 022 and GitHub ubuntu-24.04 gates independently. Local execution is uid 0 with sockets restricted; socket-dependent existing tests may return early.
- Privileged runtime remains pending VM; only build privileged tests here. Run cargo-deny on an equipped host if the wrapper skips it.

## What changed

Implemented items 2–4 within the owned files. The five daemon mount paths share a schema lookup: LVM uses `tidemark.lvm.daemon.v1`, and phantom/writers/now/hot retain `tidemark.<method>.v1`, matching their existing daemon success renderers. No success document or direct code changed.

A canonicalization ENOENT now becomes `CliError::Precondition` (exit 5), with the escaped input path and OS error. JSON writes the same schema/data/error envelope as unwatched refusals, with existing `ApiErrorCode::NotFound` (`not_found`). Other canonicalization failures and non-UTF8 API-label configuration errors retain their behavior. The `now` command resolves the filter before connecting, like `hot`; phantom/writers/lvm still resolve only after a successful daemon connection, preserving their direct fallback paths.

The LVM API reports `lvm report not configured: disabled by config` when no report is available and LVM is disabled. Enabled-but-unmeasured stays `measurement unavailable`. Watched-mount validation still runs first, and measured-empty results remain successful. The unavailable mapping still exits 4. Operator-guide text documents these errors and schemas without changing the exit-code table.

No dependency, manifest, module declaration, shared-file, measurement or attribution changes.

## Reproduction and regression evidence

Before implementation, `cargo test --offline -p tidemark-cli --lib mount_filter_tests` failed all three tests:

- `daemon_mount_contexts_are_consistent`: actual `phantom: /B`, expected `phantom: /run/tidemark/api.sock` (item 1).
- `phantom_unwatched_mapping_in_text_and_json`: actual `tidemark.lvm.v1`, expected `tidemark.lvm.daemon.v1` (item 2).
- `missing_mount_is_a_precondition`: actual exit 1, expected 5 (item 3).

Before implementation, `cargo test --offline -p tidemarkd --lib lvm_disabled_and_pending_have_distinct_reasons` failed: actual `unavailable: measurement unavailable`, expected `unavailable: lvm report not configured: disabled by config` (item 4).

Baseline logs and `baseline-regressions.patch` are in the attempt output `library/`. The blocked item-1 regression is preserved separately in `blocked-context-test.rs`, not installed as a failing or ignored repository test. Its proposed fix is unapplied and unvalidated.

After implementation, four CLI unit tests pass: the expanded missing-path test covers all five methods in text and JSON, the unwatched mapper covers all five schemas, an existing/optional/non-UTF8 path test checks unchanged behavior, and the disabled-LVM mapper checks detail and exit 4. The daemon reason regression covers disabled/enabled configuration, both unfiltered and watched `/A` requests, with fake time 120. All 134 daemon unit tests pass, including unwatched-mount and measured-empty LVM regressions. No existing integration-test expectations needed adaptation. Socket-free binary smoke runs of `now` and `hot` with a missing temporary path also emitted their `not_found` JSON envelopes and exited 5; stdout/stderr and exits are saved as `missing-*` artifacts.

## Before/after output

Let `D` mean `is not watched (watch.mounts = [...]); add it to [watch].mounts and run: tidemark deploy stage --from <file>` and `M` mean `/mnt/p13bind`. The supplied guest observation and local prefix regression establish these lines; no guest command was run here.

| Command | Before | Submitted candidate | Proposed after scope expansion (not applied) |
|---|---|---|---|
| phantom | `phantom: M: M D` | unchanged | `phantom: /run/tidemark/api.sock: M D` |
| writers | `writers: M: M D` | unchanged | `writers: /run/tidemark/api.sock: M D` |
| lvm | `lvm: /run/tidemark/api.sock: M D` | unchanged | same |
| now | `now: /run/tidemark/api.sock: M D` | unchanged | same |
| hot | `hot: /run/tidemark/api.sock: M D` | unchanged | same |

The proposed convention is the socket context already used by now/hot/lvm and by phantom/writers daemon errors without `--mount`; preserve direct contexts. It removes the repeated mount path from phantom/writers, but requires the out-of-scope dispatcher change.

LVM unwatched JSON before: `{"schema":"tidemark.lvm.v1","data":{"error":{"code":"unavailable","message":"unavailable: M D"}}}`. After: same error with `schema: "tidemark.lvm.daemon.v1"`.

Missing path before: `resolve mount: /mnt/does-not-exist: No such file or directory (os error 2)`, exit 1; JSON stdout empty. After: the same detail, exit 5, and JSON `{"schema":"tidemark.<method>.v1","data":{"error":{"code":"not_found","message":"resolve mount: /mnt/does-not-exist: No such file or directory (os error 2)"}}}` (LVM schema is `tidemark.lvm.daemon.v1`). Existing outer command/subject prefixes still apply, including the unresolved item-1 inconsistency.

Disabled LVM before: `daemon request lvm: unavailable: measurement unavailable`. After: `daemon request lvm: unavailable: lvm report not configured: disabled by config`; exit 4 in both cases. Enabled-but-unmeasured is unchanged.

## Assumptions

- The explicit “Nothing else” scope takes precedence over making an unapproved dispatcher edit. Save a concrete proposed patch for coordinator review rather than bypassing centralized error handling.
- `not_found` is the existing vocabulary for an ENOENT mount path. Other OS failures are outside the requested missing-path change and preserve their existing behavior.
- Retain daemon-absent direct fallback and explicit direct behavior, and retain watched-mount validation precedence over collector availability.
- No new privileged test is necessary for error presentation; all new regressions are socket-free and use injected time where applicable. Existing privileged suites are compiled only, pending VM.
- Use the precreated attempt branch; no branching, merging, pushing or memory-projection edits. Build offline with the host cache and pinned Rust 1.98.0.

## Validation

All commands run offline against the host Cargo cache, with `timeout 1500` per command. Full logs are in the attempt output `library/`.

| Command | Local result |
|---|---|
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --workspace --no-fail-fast` | Exit 0; 87 harness summaries: 1,028 passed / 0 failed / 14 ignored (includes nested child summaries, not unique tests) |
| `cargo build --workspace --features privileged --tests` | Exit 0; compilation only, runtime pending VM |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Exit 0 |
| `tools/ci.sh` | Exit 0; source checks passed (150 owner tags); all available checks passed |

Focused CLI: 4 passed / 0 failed. Focused daemon: 134 passed / 0 failed. Initial CLI regression run: 0 passed / 3 failed. Initial daemon regression run: 0 passed / 1 failed. Binary missing-path checks: now and hot both exit 5 with valid JSON. No privileged runtime or socket-capable gate is claimed.

Cargo-deny is not installed; the CI wrapper explicitly skipped its audit. No dependency was added. Coordinator dependency audit remains pending on an equipped host.

## Remaining uncertainty

Item 1 remains unresolved in this candidate. The proposed dispatcher patch and regression require coordinator scope authorization and validation before all task criteria can be considered met. Independent review, uid 1000 socket-capable execution, GitHub runner validation and VM runtime remain outstanding.

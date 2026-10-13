# Handoff — GATE-fix: rate history and Unix socket restriction

- Branch: existing hp-ffdd5ea5a34694760f2c8fa07a11f6fd3a0a8c04af235880f9948a7d7b8891f9
- Attempt: attempt-ffdd5ea5a34694760f2c8fa07a11f6fd3a0a8c04af235880f9948a7d7b8891f9
- Date: 2026-10-13
- Status: implementation and evidence candidate; independent verification pending.

## What changed

The mount recovery test now expects both published rates to be None at 299 seconds. The existing sample at 300 seconds now also checks both published rates are Some(1.0), matching used bytes equal to elapsed seconds. Part 1 was committed immediately as `GATE-fix: rates are absent before five minutes of history` (e03e013).

The packaged service and normative unit now say `RestrictAddressFamilies=AF_UNIX`; this is the only changed line in the packaged unit. The w6b assertion and test name, privileged w6a3 systemd-run property, ADR-020 rationale, RPM README and operator guide agree. The rationale records the guest measurements supplied by the task, not a new measurement in this attempt.

## Search results

Searched all daemon and CLI tests for fs_rate, rate_15m and rate_1h. Only the two assertions in mount_presence_recovery_remount_and_rate_history were stale. ring_eviction_and_injected_rates already expects None for short history and Some(1.0) for sufficient history; alert_source_unavailable_empty_and_measured and the remount assertions already expect absence. w6b/source.rs and CLI w4a.rs provide mock source rates, and CLI now_uses_report_rates_hot_directories_units_and_utc supplies report values without a sample history, so none require correction.

No AF_NETLINK references remain in the searched plan, packaging, daemon tests, operator guide, install harness or xtask w6c test. tests/install/install.sh contains no address-family assertion, and xtask/tests/w6c.rs uses the normative byte comparison without pinning the old families separately; neither needed editing.

## Validation

Pinned Rust 1.98.0 binaries were added to PATH and the host registry cache copied to /tmp/tidemark-gate-cargo; CARGO_HOME points to that temporary copy. All builds used --offline. Initial commands failed because cargo was absent from PATH and rustup attempted to create its home in a read-only directory; corrected commands below passed.

- cargo fmt --check: passed.
- cargo clippy --offline --workspace --all-targets -- -D warnings: passed, repeated after renaming the unit test.
- cargo test --offline -p tidemarkd --test w6a1 --test w6b: passed, 30 and 13 tests respectively. The anticipated socket failures did not occur in this attempt.
- cargo test --offline -p xtask: passed, including exact_normative_unit_and_config.
- cargo test --offline -p tidemarkd --features privileged --test w6a3 --no-run: passed; runtime pending VM.
- bash -n tests/install/install.sh and git diff --check: passed.

Logs and the rate search transcript are in this attempt's worker-output library. No product source, dependencies, shared module files, host service state or mount configuration were changed. No privileged commands were run. Full workspace tests and cargo xtask ci were not run; the assigned focused checks were used.

## Guest follow-up

On a disposable Rocky 8.8/8.10 guest after installing the candidate unit, confirm its effective setting with `systemctl show tidemarkd.service --property=RestrictAddressFamilies --value` (expected AF_UNIX). Exercise readiness, status, now, lvm, hot and a daemon scan, and check the journal for address-family errors. This runtime check remains pending VM; earlier measurements are task-provided evidence.

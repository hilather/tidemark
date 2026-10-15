# Needs from coordinator

Independent review and uid 1000 / umask 022 with sockets and GitHub ubuntu-24.04 gates remain pending. Privileged tests are build-only here; VM execution remains pending. This is an evidence candidate, not a verified integration result.

# Handoff — METRICS-off: Metrics textfile export off by default

- Branch: precreated attempt branch hp-c2fd9f43d5fbbd137264a6d0d06f0a4adf226fcf500230735474197493e5f46b
- Date: 2026-10-15

## What changed

`MetricsConfig::default()` now uses an empty path. Updated all three full configuration copies: `packaging/config/tidemark.toml`, the normative TOML block in `docs/plan/08-interfaces.md`, and `crates/tidemark-core/tests/example.toml`. Their metrics line matches the requested aligned line. The packaging comparison test additionally pins the parsed empty value. Updated only the named layout line in `02-architecture.md` and the operator guide's example and metrics instructions, including the existing collector directory prerequisite and re-staging from the controlled candidate.

No dependency, manifest, service unit, or shared module edits. The owner decision explicitly authorizes this default and corresponding normative documentation change.

## Upgrade configuration preservation

`packaging/rpm/rpm.toml:14` declares the installed `/etc/tidemark/tidemark.toml` with `config = "noreplace"`. The existing packaging test checks that declaration. `tests/install/rpm-upgrade.toml` also marks its config noreplace and takes its source from `tests/install/upgrade-config/tidemark.toml`. Left that source unchanged: it represents an existing operator-edited configuration with an explicit metrics destination, which must survive an upgrade. No packaging beyond the shipped config was changed. No RPM install/upgrade was executed here.

## Disabled component finding

No production health logic fix was needed. Status renders each component's state directly, including `disabled`. Both daemon firstboot health and deploy verification require only sampler, scheduler, api and tswriter to be `ok`; optional metrics does not enter that health predicate. `tidemarkd --check-config` validates configuration and output paths without running exporters and accepts an empty metrics path.

Regression evidence: core `metrics_textfile_is_disabled_by_default_and_when_omitted`; CLI `status_shows_metrics_disabled_plainly`; added disabled metrics assertion in `verify_missing_stale_unhealthy_healthy_and_api_failure`; daemon `check_config_accepts_metrics_disabled_by_default`. The existing exporter test `disabled_exporter_reports_why_without_reconciling_snapshots` now uses the default config without explicitly clearing the path, asserting the disabled reason and avoiding snapshot reconciliation.

## Tests

All long commands used `timeout 1500`, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, pinned 1.98.0 toolchain on PATH, and offline mode (`CARGO_NET_OFFLINE=true`; explicit `--offline` for direct build/test/lint/doc calls). Initial tool discovery needed RUSTUP_HOME to avoid a read-only default home; corrected before final validation.

- `cargo fmt --check`: passed, including final repeat after pinned-test correction (`fmt-final.log`).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed, final repeat (`clippy-final.log`).
- `cargo test --workspace --no-fail-fast`: initial run had one stale upgrade-fixture equality assertion; corrected that expectation while preserving the fixture. Focused `cargo test -p xtask --test w6c`: 14 passed. Full workspace rerun through `tools/ci.sh` passed (`ci.log`).
- `cargo build --workspace --features privileged --tests`: passed directly and in final CI; never executed privileged tests.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`: passed (`doc.log`).
- `tools/ci.sh`: exit 0, `ci: all available checks passed`; source checks, formatting, Clippy, full workspace tests and privileged feature build passed. Cargo-deny is not installed, so its audit was skipped by the wrapper.
- `git diff --check`: passed.

Logs are in this attempt's output `library/`. uid 1000/socket-enabled, GitHub runner and VM gates were not executed here. No new privileged test is required for this config default change.

## Assumptions

An empty metrics destination retains the existing disabled semantics. Explicit operator destinations remain enabled. Re-stage instructions use the guide's controlled `./tidemark.toml` candidate and existing `deploy stage --from` interface. The assigned task supersedes the general wave-task workflow; no branch creation, merge, push, privileged execution or host installation is needed.

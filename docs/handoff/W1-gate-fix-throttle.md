# Handoff — W1-gate-fix-throttle: factory handle and exact token accounting

- Branch: hp-1a963c51ae7d1c2190cefe3e83f5b0ff9018a9de067bcdf82d90e755cca64020
- Attempt: attempt-1a963c51ae7d1c2190cefe3e83f5b0ff9018a9de067bcdf82d90e755cca64020
- Date: 2026-10-11
- Status: implemented; independent verification pending submission review.

## What changed

G2: `gate_from_config` returns the approved `ScanGate` with private variant storage and the exact `gate`, `stats`, and consuming `stop` methods. Adaptive statistics come from the controller; fixed statistics report the configured rate, measured completed-acquisition blocked time, and zero changes; max statistics are all zero. Rustdoc explicitly defines zero statistics rates with profile max as unthrottled. Stop and handle drop join the controller even when a worker retains an Arc gate. Retained gates and waiting acquisitions continue at the last rate.

G7: issued/earned reservations and targets are u128 nano-tokens, with exact integer costs and carried fractional nano-token earnings. Public rates remain f64; earning rates are converted to integer nano-tokens per second. The state is constant-size, with no waiter collection. Counters rebase only when all reserved targets have retired. Admission pauses before the reservation limit, preserving every outstanding target and enough headroom for the maximum legal burst. Elapsed credit splits whole seconds and subsecond remainder before checked multiplication, avoiding intermediate overflow. Bounded statistics counters saturate instead of wrapping.

## Shared-file edits and dependencies

None. Only throttle.rs, tests/w1d/main.rs, and this handoff changed. No dependencies, manifests, lockfiles, core types, or plan files changed. No out-of-scope factory callers were found.

## Acceptance evidence

| Item | Regression evidence |
|---|---|
| G2 factory statistics and stop | `factory_final_statistics_and_waiters_after_stop`: adaptive leaves, empty-leaf fixed fallback, and max; completed throttling, live/final statistics, and waiting acquisition completion after stop |
| Handle drop joins with worker Arc retained | `factory_drop_stops_controller_with_retained_gate`: sampling stays stopped after handle drop while retained gate remains usable |
| G7 charging near 2^53 with no outstanding targets | `exact_reservations_near_float_limit_and_integer_limit`: four unit acquisitions consume real elapsed credit, seeded at 2^53 and the u128 admission limit |
| G7 outstanding target preservation | `exact_reservations_preserve_outstanding_targets`: seeded outstanding debt plus a concurrent acquisition; exact reservation checked near 2^53, admission blocks near u128 limit, seeded target retired only after credit covers it |
| Extreme elapsed arithmetic | `large_elapsed_credit_does_not_overflow_or_fill_unpaid_debt`: maximum legal rate and one trillion elapsed seconds produce exactly computed credit without incorrectly paying all debt |
| Fractional credit survives rebasing | `fractional_nanotokens_survive_accounting_and_rebasing` |
| Existing rate-change and throughput behavior | `time_weighted_debt_and_burst`, concurrent decrease, bulk recovery, and four-thread fixed-rate tests remain passing |

## Validation

All commands used the pinned Rust 1.98.0 toolchain, the host Cargo cache, and offline mode (`--offline` for Cargo checks; `CARGO_NET_OFFLINE=true` for tools/ci.sh).

- `cargo fmt --check`: exit 0.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: exit 0.
- `cargo test --offline --workspace`: exit 0; fs library 27 tests and w1d 10 tests pass.
- `cargo build --offline --workspace --features privileged --tests`: exit 0.
- `tools/ci.sh`: exit 0; all available checks passed. cargo-deny is not installed and was explicitly skipped.
- `git diff --check`: exit 0.

Final command transcripts are in this attempt's output `library/{fmt,clippy,test,privileged,ci}.log`. Initial runs identified one Clippy digit-grouping issue and an exact-f64 mean assertion; both were corrected before the final full validation run.

## Remaining uncertainty

Privileged tests were compiled, not executed: pending VM. This change adds no privileged behavior or VM-only tests. Cargo-deny audit remains pending an equipped host. Controller join timing retains the documented requirement that the supplied sampler finishes within one interval. Statistics measure blocked time for completed acquisitions; acquisitions finishing after stop's returned snapshot cannot contribute to that earlier snapshot. No task item is intentionally deferred. No host metadata, mounts, LVM, services, or configuration were changed.

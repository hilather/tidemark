# Handoff — W5-A-fix2: preserve nested-root dirty hints

## Needs from coordinator

Resolve the unrelated daemon build error at `crates/tidemarkd/src/lib.rs:757:22`: E0425, cannot find function `validate_marker_roots` in module `state_files` (the included deployment module defines `validate_marker_roots_at`). Full workspace acceptance cannot be established until that error is fixed outside this task.

- Attempt: `attempt-78437571b92ba26fe981f8611a4a0577d897661b27a004ba96ce7061dfb9070f`
- Branch: `hp-78437571b92ba26fe981f8611a4a0577d897661b27a004ba96ce7061dfb9070f`
- Date: 2026-10-11
- Status: implementation and local evidence supplied for review; independent verification and integration remain with the coordinator.

## Findings

| Finding | Status | File and evidence |
|---|---|---|
| G3: overflow loses dirty hints for nested scan roots | fixed now | `crates/tidemark-live/src/watch/mod.rs`: `overflow_keeps_nested_scan_root_dirty` failed before the fix with left `[]`, right `/data/reports`; the regression passes after the fix. `loss_acknowledgements_are_independent_in_both_orders` covers siblings, mount/child, nested parent/child in both orders, and a second loss. |

## What was built

Loss coverage is separate from ordinary dirty entries: a persistent flag plus a FIFO of roots taken since the latest loss. Clearing the FIFO on every loss is equivalent to advancing a generation, without counter wraparound. Each root inside the watched mount independently returns itself once after that loss, even if another caller has already taken its parent, child or sibling. The FIFO holds at most 1,024 RawPaths; forgetting its oldest acknowledgement conservatively reports that root dirty again. Repeated losses clear existing storage and never add entries.

Ordinary ancestor dirty entries return the requested root and remain retained for other callers. Ordinary entries at or below the requested root are returned and removed. Results remain sorted by raw path bytes with duplicates removed. Clock observations at worker and ring API boundaries now invalidate root acknowledgements on rollback, including within a minute. Public signatures and types are unchanged.

## Acceptance evidence

All new tests are unit tests in the owned watcher module, with injected times and synthetic overflow; they need no fanotify privileges or actual `/data` tree. The overflow fixture opens only a temporary directory.

| Requirement | Test |
|---|---|
| Watched `/data`, overflow, nested `/data/reports` | `overflow_keeps_nested_scan_root_dirty` |
| Siblings and parent/child in both orders; repeated loss | `loss_acknowledgements_are_independent_in_both_orders` |
| Ancestor other than mount retained, descendants removed, sorted deduplicated output | `ancestor_dirty_entry_is_retained_and_descendants_are_removed` |
| 1,024 acknowledgement bound, oldest eviction, repeated loss, outside-mount prefix | `loss_acknowledgements_are_bounded_and_forgetting_is_conservative` |
| Existing descendant drain semantics without loss | `no_loss_preserves_sorted_descendant_drain`; existing tests unchanged |
| Rollback reinstates loss independently per root | `rollback_invalidates_each_root_again` |

## Shared-file edits and deviations

No shared-file edits, dependencies, manifest/lockfile changes, unsafe code, or uid/group checks. Only the watcher module and this handoff changed.

## Validation

Offline checks use Rust 1.98.0, the host Cargo cache, and `CARGO_NET_OFFLINE=true`. Logs and exit files are in this attempt's worker-output `library/`. The pre-fix failure transcript is `reproduction.log`.

| Check | Result | Log |
|---|---|---|
| `cargo fmt --check` | exit 0 | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 101: unrelated daemon E0425 above | `clippy.log` |
| `cargo test --offline --workspace` | exit 101: same daemon E0425; compilation blocked tests | `test.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 101: same daemon E0425 | `privileged.log` |
| `tools/ci.sh` | exit 1: source checks passed (139 owner tags), then Clippy failed on same daemon E0425 | `ci.log` |
| `cargo clippy --offline -p tidemark-live --all-targets -- -D warnings` | exit 0 | `owned-clippy.log` |
| `cargo test --offline -p tidemark-live` | exit 0: 57 library tests passed; integration and doc targets passed (0 tests without privileged feature) | `owned-test.log` |
| `cargo build --offline -p tidemark-live --features privileged --tests` | exit 0: compilation only; runtime pending VM | `owned-privileged.log` |

No unrelated source was changed to bypass the workspace failure. The CI wrapper stops at Clippy, so it did not reach cargo-deny; no dependency-policy audit is claimed.

## Known limits

The bounded acknowledgement FIFO may trigger redundant rescans after eviction; it does not suppress a forgotten root. Lookup scans at most 1,024 acknowledgements. The existing dirty-entry cap remains 200,000. Retained ordinary ancestor entries deliberately report a nested root on every take until the ancestor itself is drained.

No privileged tests were executed and no workstation mounts, LVM, services or protected directories were changed. Existing watcher kernel overflow, FID and load fixtures remain pending VM; feature-enabled compilation is recorded separately. This change adds deterministic unprivileged coverage for the dirty-state policy, rather than a new kernel-runtime fixture. cargo-deny availability and any unrelated workspace failures will be reported with the required check results.

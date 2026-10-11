# Handoff — W2-A-fix: Aggregator review fixes

- **Attempt:** attempt-32d0d81dda3ef29fcad52e2aca90a55445a716663750aba6e1f91080a6930354
- **Branch:** hp-32d0d81dda3ef29fcad52e2aca90a55445a716663750aba6e1f91080a6930354
- **Date:** 2026-10-11
- **Status:** implementation and local evidence submitted for independent verification.

## Findings

| Finding | Status | Implementation and evidence |
|---|---|---|
| 1: retain every bulk single | fixed now | `agg/notable.rs::Candidates`, `agg/mod.rs::candidate`, `agg/linkset.rs::resolve`: size buckets drop losing sizes during ingestion; inode-ordered inline per-directory rows bound cutoff ties. `bounded_matches_unbounded` compares random inputs, final-ID permutations, threshold/top/cap combinations and reversed arrival against unbounded selection. `five_million_bulk_singles_retain_one_candidate` exercises actual add_single; `half_million_equal_size_walk_directories` exercises worst-case cutoff ties. |
| 2: sampled errors treated as totals | already fixed | Baseline `agg/mod.rs::add_dir_record` reads `rec.errors_total`; `add_skeleton` forwards `ns.errors_total`. Additive rollup and summary use that total. Existing `tests/w2a/main.rs::uncapped_directory_errors_survive_walk_and_namespace_sampling` checks 20,140 errors with one retained detail in both input modes. No accounting change was needed. |
| 3: arrival-dependent error emission | fixed now | `agg/mod.rs` retains the ordinal within the source record alongside each sampled detail. `agg/rollup.rs` sorts by final directory ID and ordinal after remapping. Truncation still selects by raw path/op/errno, with discovery ID and ordinal breaking identical-detail ties. `error_order_below_at_and_above_cap_preserves_record_order` compares reversed record arrivals with caps 3, 6 and 10 for six details, checks final directory order, and checks original within-record order below the cap. |
| 4: directory ceiling/layout | fixed now | No budget parameter exists in AggConfig or the constructor, so the expressly authorized fallback is `MAX_DIRS=16_000_000`, returning existing `AggError::Capacity` at the ceiling. `directory_limit_and_five_million_discovery_id` prepares sparse IDs 5,000,000 and 15,999,999 and rejects 16,000,000. Boxed slots make these holes inexpensive. `storage_layout` measures reduced node payload. |

## Candidate bounds and exact selection

Let D be the number of candidate-bearing directories and M=max_notable. Below the threshold, a directory retains at most min(top_files,M) inode-ordered rows at a particular size; at/above the threshold its bound is M. Global size buckets retain fewer than M rows strictly above the cutoff and at most D*M rows at the cutoff. Thus retained candidates are at most (D+1)*M, plus one transient inserted row, independent of the number of scanned inodes. The tighter below-threshold tie bound uses D*min(top_files,M). This explicitly accounts for the unavoidable directory multiplier while final IDs are unknown; it is not a claim that adversarial equal-size ties use O(M) memory independent of D.

A row dropped for smaller size cannot win after subsequent arrivals; within a directory/size, a row past its inode cutoff cannot win against retained rows from that same directory. All potential final-ID winners therefore survive. After renumbering, candidates drain lazily into a BTreeMap capped at M by the normative (disk descending, final directory ID ascending, inode ascending) order. Final threshold/top union truncation remains the existing unbounded selector, operating on at most M rows. Preparation does not materialize or sort the full tie pool. Names stay attached to retained walk candidates; bulk names remain absent for phase-three resolution.

The five-million-single test uses top_files=max_notable=1 and threshold=u64::MAX, checks one retained row after every insertion, and verifies the smallest inode winner and exact five-million file total. Candidate payload is 72 bytes. Two single-node BTreeMaps and inline containers are charged conservatively at **at most 8 KiB**, using the pinned Rust 1.98 eleven-slot leaf layout plus 512 bytes of header/edge slack per leaf; allocator metadata is excluded. The test checks this structural bound and that SmallVec never spills. Final selection scratch is O(M); the bound does not include directory/accounting/output buffers or process baseline.

## Directory storage

Before: Node=288 bytes and each sparse Option<Node> slot=288 bytes. After: **Node=232 bytes**, sparse Option<Box<Node>> slot=8 bytes, Totals=120 bytes, LinkAcc=128 bytes and FileRow=72 bytes. A populated directory therefore uses 240 bytes of node/slot payload before name, children, allocator metadata and vector slack. The 200-byte node target is still not reached.

Own file count and own charged bytes are derived from additive subtree totals minus immediate children's totals during output. Directory subtree count comes from the inclusive preorder interval. Apparent own charge stays explicit because subtree apparent union is not additive. Union accumulators and stamps occupy temporary 32-byte scratch slots only for scans with links and are released before notable final selection and row output. Peak preparation still includes that scratch plus numbering vectors; moving scratch out of Node is not a claim it disappears from peak memory. Box allocations add allocator overhead but prevent a single high discovery ID from allocating millions of full node holes.

The fixed limit is a documented fallback, not memory-budget enforcement or a guarantee that sixteen million populated nodes fit the service cgroup. Existing orchestrator admission policy and cgroup hard bounds are unchanged.

## Scope and contracts

Only `crates/tidemark-scan/src/agg/**`, `crates/tidemark-scan/tests/w2a/main.rs` and this handoff changed. No shared-file edits, dependencies, manifests, lockfile, core types, public aggregator signatures, CLI/config contracts, plan edits, branches, merges or pushes. No privileged/destructive commands or external messages were used. Memory projections were not edited.

## Validation

Final check results and scale measurements are recorded below. Logs and exit files are in this attempt's worker-output `library/`. Cargo uses pinned Rust 1.98, `--offline` and CARGO_NET_OFFLINE=true with a writable copy of the host registry at `/tmp/w2a-fix-cargo`.

Existing feature-gated `loop_xfs_walker_golden` remains **pending VM**; compilation only is performed here. No new privileged test is needed for metadata-only selection, storage layout or ordering fixes. Advisory/license audit remains dependent on cargo-deny availability.

| Final command | Local result | Evidence |
|---|---|---|
| cargo fmt --check | exit 0 | `fmt-final.log`, `fmt-final.exit` |
| cargo clippy --offline --workspace --all-targets -- -D warnings | exit 0 | `clippy-final.log`, `clippy-final.exit` |
| cargo test --offline --workspace | exit 0 | `test-final.log`, `test-final.exit` |
| cargo build --offline --workspace --features privileged --tests | exit 0, compilation only | `privileged.log`, `privileged.exit` |
| tools/ci.sh, offline | exit 0 on retry; cargo-deny unavailable/skipped | `ci-final.log`; initial `ci.log` preserved |
| Aggregator unit/oracle tests | exit 0 | `agg-unit.log`, `agg-unit.exit`; also final workspace run |
| Explicit 5M bulk-single scale | exit 0; 10.977 s; whole-process peak 12,004 KiB | `scale-singles.log`, `scale-singles.exit` |
| Explicit 500K-directory equal-size tie scale (2M arrivals) | exit 0; 4.673 s; whole-process peak 95,184 KiB | `scale-ties.log`, `scale-ties.exit` |
| Storage layout / whitespace | exit 0 | `layout.log`, `diff-check.log` |

Scale/layout measurements invoke the final unprivileged unit-test binary directly, avoiding compiler RSS. Measurements are from a shared x86_64 Linux development host under concurrent checks; elapsed times are diagnostic rather than performance guarantees. Regular workspace tests intentionally ignore scale tests; both new ones were executed explicitly above.

The first CI run passed source checks/fmt/Clippy, then failed outside scope in `tidemark-cli::client::tests::deadline_and_poisoned_connection` at `client.rs:328`: `Interrupted system call` rather than its expected timeout. The complete final workspace run and an isolated rerun of that test passed; the first failure remains in `ci.log`, and `cli-deadline-retry.log` records the isolated success. No CLI file was changed to conceal it.

Remaining uncertainty: VM/Rocky/XFS runtime coverage and advisory/license audit; node target remains 232 rather than 200 bytes; adversarial directory-wide size ties still require the documented D multiplier and container/name overhead; no claim that every allowed ceiling combination fits the service cgroup. These limits are evidence for review, not independent task verification.

The CI retry completed with `ci: all available checks passed` (exit 0). Cargo-deny was unavailable and explicitly skipped; dependency advisory/license auditing remains pending. The final explicit workspace run counted 368 passed tests and 10 intentional scale ignores. Both new scale tests were also executed directly and passed.

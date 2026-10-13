# Handoff — W7-A2-store: paged snapshot queries

## Needs from coordinator

Independent review and integration remain pending. No existing index covers any requested child sort order or the link instance order; million-child responsiveness is not established. A covering-index schema proposal, if needed, belongs to the coordinator. Privileged runtime and cargo-deny remain subject to the validation limitations below.

## What changed

Added `Snapshot::children_count`, `children_page`, and `link_instances_page` beside existing reader methods, without changing existing signatures, schema, dependencies or other crate behavior. Child pages use exclusive metric/name keysets, raw-byte substring matching, ascending raw-name metric ties and selectable primary direction. Reverse traversal returns the preceding rows in reversed order; without a cursor it returns rows from the end. Page allocations are bounded by the existing 1,000,000-row cap; zero limits return empty results.

Link pages return `(rowid, LinkInstanceRow)` cursors, ordered by directory ID, nullable byte name and rowid. This preserves repeated bulk unresolved instances and repeated named rows individually. Unsigned cursor keys and inode inputs use checked SQLite conversions.

## Draft review

Kept the draft API shapes, byte filtering, reverse traversal and cap checks. Fixed the end-of-block statement borrow lifetime by collecting before returning. Removed cursor-presence OR guards from the generated SQL and omit the cursor predicate on first pages. Link comparisons explicitly include the nullable-name discriminator so NULL ordering agrees with ORDER BY; rowid remains the final tie-break. Selected explicit link columns rather than relying on `SELECT *` column position. The child CTE selects bounded IDs before fetching full directory rows.

Expanded draft coverage into three tests with bounded test loops: every sort/direction/filter, every possible cursor (including ties), reverse and end reads, complete row comparisons, invalid UTF-8, repeated NULL and named links in the same directory, missing parents/inodes, zero limits, row caps and overflowing unsigned cursor keys. Added the required 2,500-child fixture with 1,000/1,000/500/0 page lengths.

## Scope and shared-file edits

Only reader additions, `tests/storage.rs` additions and this handoff changed. No shared manifest/module edits, dependencies or contract changes. Existing file owner headers retained. No branches created, merges, pushes, privileged commands or host configuration writes.

## Assumptions

- As in a valid filesystem snapshot, direct child names are unique within a parent; metric plus raw name is a total key. Root is not its own child.
- Cursors belong to this immutable snapshot and must reuse parent/inode, sort, direction and filter. They are not portable between snapshot files; rowids identify duplicate link instances within one file.
- All existing SortKey variants, including Apparent and Inodes, are supported for consistency with the existing reader.
- No new privileged test is needed for pure immutable SQL queries; existing privileged targets are compiled, runtime remains pending VM.
- Cargo uses the cached pinned 1.98.0 toolchain and offline dependencies. Canonical attempt inputs and receipts were read; no delivered update was pending. Memory projections were not edited.

## Index and performance limits

`dirs_parent(parent_id)` and `li_ino(ino)` narrow candidates. None of Disk, Apparent, Reclaimable, Charged, Files, Inodes, Newest or Name has an index covering parent plus key/name. Link order `(dir_id,name,rowid)` likewise lacks an inode-prefixed covering index. SQLite can select existing narrowing indexes (planner dependent) but must sort candidates; keyset paging avoids growing OFFSET discard costs, not repeated candidate scans/sorts. No schema change or million-child latency claim.

## Acceptance evidence

- `w7a2_children_keysets_match_all_orders_and_filters`: all eight keys, both directions, raw filters (empty, multi-byte, invalid UTF-8, absent), count parity, full row round trips, forward/backward paging and each exclusive cursor. Legacy reader parity for its native directions, independent sorted fixture oracle for other directions.
- `w7a2_large_children_pages_have_no_duplicates_or_omissions`: 2,500 children, pages of 1,000; exact full-row comparison with existing children reader and count 2,500.
- `w7a2_link_pages_preserve_null_duplicates_and_byte_names`: same-directory duplicate unresolved/named links, invalid bytes, full legacy-reader parity, every cursor in both directions, end reads and bounds.
- Focused paging run: 3 passed, 0 failed (0.42s); committed immediately after passing.

## Validation

Final command results will be appended after all required checks finish. Full logs are in the canonical attempt output `library/`. Every long command is bounded by `timeout 1500`; Cargo checks use offline mode. This handoff is implementation evidence, not independent task verification.

| Required check | Result | Log |
| --- | --- | --- |
| `cargo fmt --check` | exit 0 | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0; 697 passed, 0 failed, 14 ignored | `tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only | `privileged.log` |
| `tools/ci.sh` with offline Cargo environment | exit 0; all available checks passed; cargo-deny absent and explicitly skipped | `ci.log` |

CI test summaries also total 697 passed, 0 failed and 14 ignored. Some existing operator tests can skip transport scenarios in restricted environments; harness passes are not claimed as VM/socket acceptance. No assertion was weakened or unrelated source modified. All three new paging tests passed in both aggregate runs. The earlier development invocation found a statement lifetime compile error, fixed before the focused passing commit. Initial Cargo invocation required selecting the cached pinned toolchain explicitly because the worker default Rust home is read-only.

## Remaining uncertainty

Independent review/integration, cargo-deny on an equipped host, existing privileged runtime gates and million-child performance measurement remain pending. No privileged runtime behavior is introduced by these SQL reader additions. No verified release/task-success declaration is made.

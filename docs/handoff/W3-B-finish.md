# Handoff — W3-B-finish: streamed hard-link topology

- Attempt: `attempt-52cd9c4aae92ef15a85cc00e7bbb91423abc31bd1e5944535aefa43a0291fac4`
- Branch: `hp-52cd9c4aae92ef15a85cc00e7bbb91423abc31bd1e5944535aefa43a0291fac4`
- Date: 2026-10-11
- Status: implementation evidence candidate; independent verification pending.

## What changed

`Regions::build` streams directory intervals in preorder and retains only selected regions. Both `crossing_sets` and `link_report` use two read-only, inode-ordered SQL cursors merged by inode through `Snapshot::conn()`. A reusable flat buffer holds at most one million directory IDs for the current inode; no global candidate or instance collection is built. Results preserve allocated-byte descending, LCA path byte ascending, inode ascending order, and representative directories use the smallest ID. Totals remain independent of the result limit. Largest/crossing results, region pairs, pinned directories, region selection, and per-inode ancestor tracking are explicitly bounded. Shared-directory selection uses a bound SQL limit.

The set cursor forces the inode primary-key scan outermost with `NOT INDEXED CROSS JOIN`; the instance cursor scans the existing inode index. Merging avoids a per-set instance-index lookup and a global joined-row sort. Narrow subtree queries trade scans of the inode and instance tables for bounded memory and simple ordered cursors. All values are bound parameters; no snapshot writes, attachments, dependencies, or shared-file edits were added.

Bulk `--show-paths` now calls `read_dir_entries_checked` with the snapshot root, device, instance-directory path and expected directory inode, and the gate. Missing directories, identity/device/mount mismatches, and symlink replacements are rendered `(gone)`. Other I/O errors still propagate. Names stored in walk snapshots retain snapshot status.

## Acceptance and existing test coverage

| W3-B item | Evidence | Status |
|---|---|---|
| `tidemark links /data --boundary-depth 2` on a 1M-set synthetic snapshot in <10 s | `million_set_cli_timing`, ignored scale test, measures CLI process/output/JSON decoding after setup; uses a temporary root in place of `/data` | Met locally in release; timings below |
| `crossing_sets` and `Regions` rustdoc examples | Existing examples retained; `cargo test -p tidemark-store` includes doctests | Met locally |
| Nested/depth regions and brute-force containment | Existing `nested_globs_and_depth_are_relative`, raw-byte glob test, and `intervals_match_bruteforce` retained | Met locally |
| Scanned hardlink web crossing counts | Existing `scanned_hardlink_web_crossings_and_paths` retained | Met locally |
| Normative worked example | Existing `link_semantics_basic_golden` retained | Met locally |
| Symlink lexical/checked fixture classification | Existing FS W3-B integration tests retained | Met locally |
| Cross-filesystem targets and mount exclusion | Existing `other_filesystem_target_and_mount_boundary_pending_vm` | Pending VM; compile only |
| Same-device bind mount exclusion | Existing `same_device_bind_mount_is_skipped_pending_vm` | Pending VM; compile only |
| Streamed rows and bounded intermediate storage | Two inode-ordered cursors and reusable per-set buffer; streamed selected directory intervals | Implemented; local evidence below |
| Bulk path resolution stays within scanned filesystem | Extended `bulk_show_paths_marks_live_and_gone` covers replacement inode and symlink replacement | Met locally |

`streamed_selection_matches_reader_order_and_subtree_scope` additionally compares largest-set and crossing selection against the existing reader, for three report roots and four limits, including zero; it checks LCA paths, stored-instance counts, and crossing totals.

## Measurements

Final synthetic fixture: 1,000,000 complete two-instance sets, depth-2 boundaries and five directories. Snapshot construction is outside each measured interval. Scale tests remain `#[ignore = "scale"]` and were explicitly executed.

| Final measurement | Time | Command / evidence |
|---|---|---|
| Library report, debug | **9.818890308 s**, assertion passed | `cargo test --offline -p tidemark-store --test w3b million_set_report_timing -- --ignored --nocapture`; `library/scale.log`, exit 0 |
| Library report, release | **3.261418952 s**, assertion passed | Same command with `--release`; `library/scale-release.log`, exit 0 |
| Full CLI, release | **1.137351239 s**, assertion passed | `cargo test --offline --release -p tidemark-cli --test w3b million_set_cli_timing -- --ignored --nocapture`; `library/cli-scale-release.log`, exit 0 |

The CLI interval includes command launch, snapshot selection, report generation, JSON rendering, output collection and parsing. It uses the existing default top limit of 25. The CLI fixture's directory rows come from an unprivileged scan; the million link sets are synthetic.

Visible hardware: Intel Core i7-8750H @ 2.20 GHz, x86_64, 12 logical CPUs. These are local sandbox observations, not VM results or a performance guarantee. The final debug result has little headroom against 10 seconds. Earlier joined-row implementations measured 30.969352631 s in debug, then 17.172674374 s after avoiding the joined-row sort, and 10.821747584 s in release while CLI compilation was active. Their failed assertions are retained in `scale-first-debug.log`, `scale-join-debug.log`, and `scale-first-release.log`; none describes the final two-cursor implementation. The original W3-B handoff recorded 85.079075606 s for the interim implementation.

## Validation

| Check | Exit / result | Evidence in `library/` |
|---|---|---|
| `cargo fmt --check` | 0 | `fmt.log`, `fmt.exit` |
| `cargo clippy --workspace --all-targets -- -D warnings` | 101, out-of-scope W1-B test errors | `clippy.log`, `clippy.exit` |
| `cargo test --workspace` | 101, same errors | `test.log`, `test.exit` |
| `cargo build --workspace --features privileged --tests` | 101, same errors | `privileged.log`, `privileged.exit` |
| `tools/ci.sh` | 1, stops at workspace Clippy with same errors | `ci.log`, `ci.exit` |
| `cargo test -p tidemark-store --test w3b -p tidemark-fs --test w3b -p tidemark-cli --test w3b` | 0; 14 passed, 2 scale tests ignored | `owned-tests.log`, `owned-tests.exit` |
| `cargo test -p tidemark-store` | 0, including property tests and four doctests | `store-tests.log`, `store-tests.exit` |
| Store all-targets Clippy, FS lib/W3-B Clippy, CLI lib/bin/W3-B Clippy | 0 each, with `-D warnings` | `store-clippy`, `fs-clippy`, `cli-clippy` logs and exit files |
| `cargo build -p tidemark-store -p tidemark-fs -p tidemark-cli --features privileged --test w3b` | 0; compilation only | `owned-privileged.log`, `owned-privileged.exit` |

Workspace failures are at `crates/tidemark-fs/tests/w1b/main.rs:15` (missing `WalkConfig.max_record_bytes`) and lines 599/726 (`WalkError::Limit` patterns use string literals although the payload is now `String`). These files are outside this task's scope and remain unchanged. CI cannot proceed to its remaining gates after Clippy fails.

Commands use Rust 1.98.0, `CARGO_HOME=/tmp/tidemark-w3bf-cargo` (a private copy of the host registry cache), and `CARGO_NET_OFFLINE=true`; Cargo commands explicitly use `--offline` where applicable. Full logs and exit files are in the canonical attempt output `library/`.

## Known gaps reviewed

- Interim streamed-access gap is closed in owned code; `Snapshot::file()` is unnecessary for a single-snapshot query.
- Symlink target membership remains lexical even with target stat. Same-filesystem secondary symlink escapes can still appear InsideRoot. The specified stat-only algorithm supplies no canonical target path; a stronger gated resolver requires a coordinator contract decision. Not changed silently.
- O_NOATIME EPERM fallback remains implemented, with no independently forced fallback test. Not done; UID/permission assumptions were avoided.
- VM execution remains pending. No privileged runtime commands were run.
- `link_instances = none` remains explicitly rejected because topology cannot be reconstructed from missing instances.
- Live bulk resolution remains a moment-in-time view with existing one-million-entry/64-MiB path-output caps. Non-identity I/O failures still propagate.
- Incomplete-set ancestor accounting still uses per-directory reader queries; only incomplete sets incur this cost. The measured scale fixture consists of complete sets, as in the retained test. General incomplete/deep-tree performance is not established.

## Needs from coordinator

Resolve the out-of-scope W1-B test compile failures listed in validation before claiming workspace CI is green. No plan, core contract, manifest, crate root, reader, or other task's file was changed.

## Definition of Done evidence

Only owned paths changed; no new dependencies or unsafe code. Public rustdoc examples and existing tests retained. Privileged W3-B tests remain feature-gated and pending VM. Workspace limitations and remaining uncertainty are recorded rather than claimed as passing.

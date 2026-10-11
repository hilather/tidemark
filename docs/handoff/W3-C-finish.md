# Handoff — W3-C-finish: streamed ncdu export and atomic publication

- Branch: hp-da307a29027a30b2ad2a395e03f969060699cfbd4bb370b84211b5d24de18962
- Attempt: attempt-da307a29027a30b2ad2a395e03f969060699cfbd4bb370b84211b5d24de18962
- Date: 2026-10-11
- Status: implementation evidence candidate; independent verification and VM gate pending.

## What changed

Removed the interim 4096-file subtree collection and error. `export_ncdu` now reads directories through a bound preorder SQL cursor and named single-link notable files through per-directory cursors on `Snapshot::conn()`. Notable files retain byte-name/inode ordering, checked accounting and raw/lossy escaping. Multi-link and unnamed files remain in the synthetic aggregate; depth truncation retains additive totals. No snapshot writes, live-file resolution, new dependencies, schema changes or public interface changes.

The reader sets `temp_store=MEMORY`. To bound SQLite sorting as well as Rust storage without changing that shared connection setting, notable queries use 256-row keyset pages with a bound LIMIT. Only the last key and the current borrowed row are retained; name buffers are reused within pages. There is no total notable-file cap. The existing 65536-open-array depth bound remains. The frozen schema lacks a name-order index, so large notable directories require repeated indexed directory scans; this is a time/memory tradeoff, not a claim of linear notable-file sorting time.

CLI `create_output` stages an exclusively created 0600 `.tidemark-export-*.tmp` file in the canonical destination directory. `run` completes export and flush; `publish_output` syncs the file and uses `renameat_with(NOREPLACE)`. Failed exports remove their temporary file on drop; process interruption may leave a conspicuous temporary file but cannot publish a partial destination. Existing files, symlinks and destinations appearing during export are refused. State-directory protection, stdout export and wide rendering retain their behavior.

## Shared-file edits

None. Only four owned Rust files and this handoff changed; no manifests, lockfiles, shared module declarations, core contracts, plans or memory projections changed.

## Tests and acceptance evidence

| W3-C acceptance or required test | Evidence | Status |
|---|---|---|
| Export loads in ncdu 1.x/2.x on VM | Existing feature-gated `vm_ncdu_import_reexport` retained; no privileged runtime executed | pending VM |
| Metadata note states charged/additive view | Existing `nested_arrays_subroot_depth_and_charged_totals` | met locally |
| Link fixture structure and sum equals measured root charge | Existing `scanned_link_fixture_additive_export` | met locally |
| Non-UTF-8 raw preservation and lossy JSON | Existing `scanned_nonutf8_preserved_or_lossy_and_escaped` | met locally |
| 1M-directory streaming RSS delta below 64 MiB | Existing scale test rerun; measurement below | met locally |
| More than 4096 notable files in one directory | Updated former cap regression `notable_files_above_old_cap_stream_and_io_errors_propagate`: 4097 files, byte order, additive total, empty residual and identical snapshot bytes | met locally |
| Export errors propagate and absent roots fail | Retained error checks in former cap regression | met locally |
| Atomic/private output, overwrite and state protection | Extended `export_mode_overwrite_state_protection_and_wide_json`: malformed snapshot fails after streaming starts; no target or temporary remains | met locally |
| Destination collision during export preserves existing file | New unit `staging_is_private_and_publication_never_replaces_a_racing_target` | met locally |
| Wide columns, disk/path and app-owner/btree/ls hint | Existing CLI regression retained | met locally |

Existing tests were retained, with the obsolete cap expectation replaced by successful streaming assertions. Synthetic rows use abstract byte values; scanned fixture totals are measured rather than filesystem-dependent literals. No privileged commands ran on this host.

The ncdu format page was accessible during this attempt: https://dev.yorhel.nl/ncdu/jsonfmt. Its array/infoblock structure and byte-name conventions agree with the retained export representation. This source inspection does not replace VM import testing.

## Validation and measurements

Final logs, exit codes and elapsed command times are under the attempt's worker-output `library/`. Cargo uses cached Rust 1.98.0, `/tmp/w3c-cargo` copied from the host registry, `CARGO_NET_OFFLINE=true` and explicit `--offline` where supported. Initial compilation exposed row-conversion errors in the edited exporter; these were fixed using the existing checked SQL integer helpers before final validation.

| Command | Result / elapsed |
|---|---|
| `cargo fmt --check` | 5 seconds, exit 0; `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 77 seconds, exit 0; `clippy.log` |
| `cargo test --offline --workspace` | 196 seconds, exit 101; `test.log` |
| `cargo build --offline --workspace --features privileged --tests` | 133 seconds, exit 0; `privileged.log` |
| `tools/ci.sh` | 200 seconds, exit 1; `ci.log` |
| `cargo test --offline -p tidemark-store` | 135 seconds, exit 0; `store.log` |
| `cargo test --offline -p tidemark-cli --lib --test w3c` | 84 seconds, exit 0; `cli-owned.log` |
| `cargo test --offline -p tidemark-store --test w3c million_directory_export_rss_below_64_mib -- --ignored --nocapture` | 296 seconds, exit 0; `scale.log` |

Store tests: 50 passed, 0 failed, 4 scale tests ignored in the ordinary run (including doctests). CLI library and W3-C integration tests: 64 passed, 0 failed. Workspace tests stopped at the two W2-C failures; 108 passed, 2 failed and 2 ignored before stopping. CI's 135 owner tags, forbidden-symbol scan, format and Clippy steps passed; tests stopped CI before cargo-deny. The initial focused CLI run also passed (182 seconds including cold build/lock wait, tests 0.59 seconds). Final W3-C store regression suite completed in 0.55 seconds; final CLI W3-C tests took 0.42 seconds. Command times include compilation and build-lock waits. No failures in owned files remain in these checks.

Scale measurement: 1,000,000 directories, sampled RSS delta **0 bytes**, export elapsed **213.596206269 seconds**, test elapsed **295.29 seconds** (296 seconds for the command). Existing MemorySink samples RSS every MiB of emitted output and once after export; this is a sampled peak, not a continuous high-water mark. The synthetic tree is flat and contains no notable files. Debug build, Linux 7.2.5-3-omarchy, 12 reported CPUs. These results do not establish target-VM throughput or high-volume notable-directory performance.


## Known gaps closed and remaining

- Full notable streaming above the interim cap: closed using accepted connection access.
- One directory lookup per preorder id: closed with one preorder cursor.
- Partial destination after export failure: closed with temporary staging and non-replacing rename.
- ncdu import/export, Rocky Linux 8 and XFS runtime behavior: pending VM.
- Workspace tests and CI fail in unowned `crates/tidemark-cli/tests/w2c/main.rs`: `sigint_records_failed_and_leaves_no_snapshot` (line 572) observes `running`, expects `failed`; `sigint_keep_partial_publishes_partial_and_is_not_latest` (line 588) observes `running`, expects `partial`. Both failures recur in CI. No root cause is established and those files were not changed.
- CI stops before cargo-deny, so dependency advisory/license checks are not evidenced by this attempt.
- The existing open-array depth cap, max-depth accounting and unnamed bulk-file aggregation remain as documented in W3-C.
- Large notable directories trade repeated scans for bounded in-memory sorting. Target-VM performance is not established.
- Power-loss durability of the destination directory entry is not claimed; file data is synced before rename, with no directory fsync added.
- Submission and local results are evidence candidates, not independent verification or wave-gate clearance.

## Definition of Done

- Owned paths only; no shared-file edits or dependencies.
- Format and workspace Clippy passed; task-specific tests passed.
- Required workspace tests and CI attempted; blocked by the unowned failures listed above.
- Privileged suite compiled; runtime acceptance pending VM.
- Public documentation and owner headers retained; no production unwraps or unsafe code introduced.
- Local acceptance evidence recorded; independent verification pending submission.

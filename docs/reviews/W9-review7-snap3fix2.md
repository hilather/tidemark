VERDICT: READY
BLOCKER 0 / SHOULD-FIX 0 / NOTE 1

Reviewed exact candidate `879a64aff338ca88087f25f4681dc006fec31fa2` against `798690fae6beedbc65136e2ce24ef8f6807f6e9e`, using supplied integration checkout `6a488a47d4a2377bdfc1de27a6d70d8437fb5e13`. Candidate and integration have identical tree `4d4318e7bea5ef2589c044c1745563997055c6b0`, so the runtime evidence applies to the exact candidate. Read both previous reviews, `15-snapshot-v2.md`, `SNAP-reader2-fix2.md`, AGENTS and relevant storage/link contracts; examined the requested latest diff/log and skimmed the whole wave delta from `cab1bdc`. This is a review recommendation and evidence candidate, not release acceptance.

S2, S3 and N2 are addressed, with independently reproduced before/after sensitivity:

| Item | Candidate evidence | Old-code comparison |
|---|---|---|
| S2: TEXT payload allocation | Both queries use a lazy type-guarded `length(data)` expression; malformed rows remain visible for interval validation. The original 64 MiB TEXT root probe returns `Invalid` at 10,096 KiB VmHWM. Later-chunk projected wide/subtree probes return `Invalid` at 10,556 / 10,728 KiB. | Original root probe reaches 75,364 KiB on the `798690f` reader. The exact new repository memory regression fails old code at 141,224 KiB against its 40 MiB limit; it passes current code. |
| S3: wrong-typed required metadata | The original BLOB update, `UPDATE meta SET value=x'32' WHERE key='plain_depth'`, returns `Invalid` naming the key on the unchanged schema; the corresponding `hash_width` update does too. Additional BLOB/NULL/integer/real/invalid-UTF8/nonnumeric cases also return `Invalid` naming the key. Genuine SQLite operation errors retain `Sql`. | Exact new regression and original BLOB-meta probe fail old code with `Sql(InvalidColumnType(...Blob))`; both pass current code. |
| N2: deletion regression sensitivity | The fixture changes reserved hot-header byte 80, explicitly checks that cleanup preserves the hot/cold pair, then requires deletion of both forms and quarantine. The unchanged test passes current code in both catalog-pointer directions. | With the pre-fix `d7cd4f0` orchestrator, the exact test passes the preservation check then fails its deletion assertion at `cold_retention.rs:288`. A separate cold-pointer-only variant fails there too. |

Allocation fixtures are created outside the measured child. Independent 64 MiB BLOB probes at the root and later chunks still reject before materialization, with VmHWM 10,020–10,556 KiB. No OOM claim is made. The new non-BLOB type-only rejection test passes old and current code; it is a regression guard, while the memory test distinguishes the allocation fix. The repository subtree call reaches the projected guard through its shallow root row; the extra later-chunk probes independently exercise both projected operations with a valid root.

Before/after harnesses were compiled with Rust 1.98.0 against the current offline-built dependencies. The old store build substitutes only `reader_v2.rs` from `798690f`; the old scan build substitutes the `d7cd4f0` orchestrator. These are isolated module comparisons, not full baseline workspace builds. Exact sources, compiler arguments, selected dependencies, results and cleanup records are retained under `library/reader/` and `library/cold/`. Deliberate baseline failures are separate from the workspace gate.

No regression was established in the earlier fixes:

| Earlier item | Evidence at the candidate |
|---|---|
| B1 overlap/gaps | Neighbor checks still run before cache admission; projected ranges retain contiguity and endpoint checks. Cold/warm/range overlap and gap regressions pass. |
| S1 shallow subtree selection | Eligible shallow subtree selection remains in SQL; the deep-chunk sentinel passes. Materializing selected paths can still fetch ancestor chunks. |
| S4 selected scan resurrection | CLI and automatic deletion still remove both published forms and both quarantine siblings through trusted directory helpers. Both pointer directions have passing regressions; retained scans are not selected for these deletions. |
| S5 listing unavailable sizes | Text/JSON listing retains rows, unknown sizes and stderr diagnostics for missing files and unreadable cold headers. Refresh remains bounded to one catalog lookup. CLI cold tests pass. |
| S6 historical fixture | The historical fixture remains unchanged. The comparator removes the new cold-default comments, and config tests preserve historical defaults. |
| N1 quarantine contract | The normative text now agrees with selected-scan deletion of published/quarantine forms and protection of retained scans' quarantine leaves. |

The independent valid-input matrix passed 75 writer combinations: five topologies including root-only, chunk sizes 1/2/3/7/4096, and plain depths 0/2/4. It compares complete v1/v2 rows across all eight sorts, root/mid/last selections, relative depths and zero/small limits. The workspace reader suite also exercises 50k-directory, deep-child, byte-path, hash-collision and paging cases. No valid-writer rejection or ordering regression was found. This is correctness evidence, not the representative latency benchmark.

The broader architecture skim found no additional supported defect. Production scans still write v1. Deferred v2 file/link/diff/export operations deliberately reject before legacy SQL; the reader work does not enable production v2 writing. Cold publication retains compression/fsync, non-replacing rename, durable catalog update, then trusted unlink/fsync. No implementation, dependency, module declaration or repository test was edited by this review.

NOTE N3 — One unreproduced LVM assertion failure occurred in the first workspace run. Reference: `finding:w9-review7-snap3fix2-n3`; location: `crates/tidemark-fs/tests/w4a/lvm.rs:112`. The fake reporting command `head -c 20000000 /dev/zero` did not match the expected `Err(LvmError::Invalid(_))`; the assertion does not print the actual returned value. The implementation and test are byte-identical at `cab1bdc`, `798690f` and this candidate. A focused Cargo rerun passed, and twelve standalone executions of the unchanged command logic returned the expected output-cap error. The initial cause remains unestablished; no candidate regression, timeout or environmental explanation is asserted. Retained evidence: `workspace-tests.log`, `lvm-cap-rerun.log`, and `wave/lvm-cap-probe*` / `wave/lvm-source-comparison.txt`.

The complete default-harness workspace retry is clean. The first run was not; N3 remains a reproducibility note. No source, feature selection, test filters or thread-count setting changed between those workspace runs. The retry had no concurrent review build or probe work.

All Cargo builds/tests used `--offline`, Rust 1.98.0, the installed host dependency cache, and isolated target `/tmp/tidemark-review-d09e6c2d-target`. Setup details are retained in `validation-setup.json` and `wave/environment.txt`.

| Validation | Observed result | Evidence |
|---|---|---|
| First `cargo test --offline --workspace --no-fail-fast` | Exit 101; 1,114 passed / 1 failed / 14 ignored | `workspace-tests.log`, `workspace-summary.json` |
| Full workspace retry, same command | Exit 0; 1,115 passed / 0 failed / 14 ignored | `workspace-retry.log`, `workspace-retry-summary.json` |
| Reader / store cold / scan cold-retention / CLI cold | 18 / 25 / 4 / 3 passed in each full run | Workspace logs |
| Config-load / historical `xtask` w6c | 10 / 14 passed in each full run | Workspace logs |
| `cargo fmt --check` | Exit 0 | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 | `clippy.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only | `privileged-build.log` |
| Required cleanup | `cargo clean --offline`, exit 0; standalone binaries/old libraries removed | `cargo-clean.log`, per-probe cleanup records |

Each workspace total sums 93 completed harness summaries, including nested subprocess summaries; these are not counts of distinct test functions. The 14 ignored tests are existing scale tests. All current reader and deletion probes pass; expected old-code failures are documented separately above.

No privileged execution or Rocky VM test was performed (**pending VM**). The §8 2M-directory latency/RSS gate, full `cargo xtask ci` wrapper, cargo-deny audit, and independent documentation build were not run. No separate crash/power-cut campaign beyond existing workspace tests or deterministic concurrent publication/list refresh test was added. No branch creation, merge, push, source-checkout write or memory-projection edit occurred. Only this review report changes the repository.

Evidence directory: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-d09e6c2d758c56322c2cd8c9c5f3b5736b1b63577e102fa5aa090a222d139166/library/`.

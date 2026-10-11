**CHANGES REQUESTED — 1 BLOCKER, 3 SHOULD-FIX, 0 NIT.**

Reviewed candidate `ec23c9e8c3038fd8b7804b1ed9ea20dccd457544`, diff from `42eb6c35dafd8a88f79622032f06bf326d04ef95`. All source line references below refer to that candidate. The assigned checkout starts at the later `df21b52c1158a90925e5345132888d1d8d0de332`; builds and probes used an unchanged archive of the exact candidate under the checkout's ignored `target/w2a-review-candidate/`. The later integration's error-total fix is identified below, but was not substituted into the reviewed candidate.

**Findings, most serious first**

1. **BLOCKER — Every bulk single inode is retained as a notable candidate, regardless of selection limits.** `crates/tidemark-scan/src/agg/mod.rs:349`, `:265`; `crates/tidemark-scan/src/agg/notable.rs:15`. Reference: AGENTS rule 10; `01-requirements.md` NFR-4; `04-scanner.md` §7; W2-A bounded-memory scale requirement. Ref: `finding:w2a-candidate-retention`.

   `add_single` always pushes a 72-byte `FileRow`. The vector grows to the 32-million-item ceiling; only `prepare` sorts and truncates it. Thus `top_files=1`, `max_notable=1`, and `big_file_threshold=u64::MAX` still retain every input single inode. A realistic bulk scan with 30 million such inodes needs 2,160,000,000 bytes (2.012 GiB) for candidate payload alone, exceeding the service's 2 GiB limit before namespace data, directory nodes, allocation slack, sorting scratch, or the daemon's other state. The memory estimate in `04` §7 does not include this per-inode candidate pool.

   Independently compiled exact-candidate probes returned the correct single winner but measured the following process peaks; compiler memory is excluded:

   | Bulk single inputs | Arrival order | Peak RSS |
   |---:|---|---:|
   | 10,000 | inode ascending | 8,068 KiB |
   | 1,000,000 | inode ascending | 73,092 KiB |
   | 2,000,000 | inode ascending | 143,192 KiB |
   | 2,000,000 | shuffled | 213,428 KiB |

   The supplied 30-million-entry scale test (`tests/w2a/main.rs:304`) puts all entries into `OwnStats` and leaves notable vectors empty. Its successful memory measurement does not exercise this path. Walk mode can also accumulate many worker-prefiltered candidates across directories.

   **Recommended change:** bound candidate retention during ingestion using exact selection rules and accounted memory, preserving final-directory tie order and threshold/top-N semantics. Obtain an explicit contract decision if a budget seam is necessary. Avoid materializing and sorting all single inodes. Add candidate-heavy bulk and equal-size walk scale regressions with tiny selection limits; include sorting scratch in the measurement.

2. **SHOULD-FIX — The exact candidate reports sampled error-detail counts as total errors.** `crates/tidemark-scan/src/agg/mod.rs:140`, `:297`. Reference: `03-link-semantics.md` §3 (`t_errors`); `05-storage.md` §1 (`errors`, `t_errors`); W2-A error accounting. Ref: `finding:w2a-error-totals`.

   `add_dir_record` takes `rec.errors.len()` as the direct count, and `add_skeleton` forwards the same sampled details. The candidate walker globally caps details at 10,000 (`tidemark-fs/src/walk/worker.rs:293`). Its passing `stored_errors_are_capped_but_totals_are_not` test demonstrates 10,017 errors with only 10,000 details. Any later directory's omitted errors become zero in its row, and root totals are understated. A standalone probe of the resulting sampled records confirms root `t_errors=10000` and child `errors=0`, despite the scenario's 17 additional child errors. This can hide the incompleteness marker on that subtree.

   **Contract context:** the candidate's core types do **not** contain `errors_total`; W2-A correctly raised pending `docs/handoff/scr-W2-A/SCR-001.md` instead of changing frozen fields. This is an unresolved candidate accounting gap, not an allegation that an existing field was ignored. Inspection of later `W2-integ` commit `17d43e8` and the assigned checkout shows the added `errors_total` fields, consumers, and a 20,140-error regression. That later fix is outside this candidate diff.

   **Recommended change:** include the coordinated error-total contract/producer/consumer fix when accepting this implementation, with regression coverage for multiple directories after detail exhaustion in both walk and namespace inputs. No additional change to the already-fixed checkout is requested for this finding; its later implementation was inspected, not independently rebuilt here.

3. **SHOULD-FIX — Error-row order depends on directory arrival when the detail cap is not exceeded.** `crates/tidemark-scan/src/agg/mod.rs:249`, `:592`. Reference: `03-link-semantics.md` §3 invariant 6; `04-scanner.md` §2.3; W2-A shuffled-record equality. Ref: `finding:w2a-error-order`.

   Sorting happens only in `trim_errors`, called when the retained count exceeds the cap. Preparation translates IDs without sorting, and `write` emits the vector unchanged. With cap 10, a root error at `/r/z` and a child error at `/r/a/z`, arrival `[root, child]` emits `[(0,/r/z),(1,/r/a/z)]`; arrival `[child, root]` emits the opposite sequence. The independently run probe confirms identical directory rows but unequal error rows. The existing error-order test uses six errors with cap five, which forces sorting and misses the defect.

   **Recommended change:** deterministically order the retained error sample before emission even below or exactly at the cap. Use raw path, operation, errno, and a stable attribution tie-break where needed. Add below-cap and exactly-at-cap record-permutation regressions; retain deterministic selection when truncating.

4. **SHOULD-FIX — The fixed directory ceiling excludes the documented five-million-directory workload.** `crates/tidemark-scan/src/agg/mod.rs:43`, `:129`. Reference: `01-requirements.md` §2 scale envelope (0.5–5 million directories); W2-A node/storage requirements. Ref: `finding:w2a-directory-cap`.

   Every discovery ID at or above 4,000,000 is rejected, independently of available or configured memory. A five-million-directory tree necessarily crosses this ceiling; Gone holes can reach it with fewer surviving directories. A small probe confirms that adding root plus `DirId(4_000_000)` yields a capacity error before allocating sparse slots. The two-million-directory test cannot detect this restriction.

   **Recommended change:** support the documented upper envelope with an explicit, memory-accounted directory limit and suitable storage, or seek an approved reduction of the supported envelope. Add a boundary test for that decided limit. Raising the constant alone is insufficient given the node footprint below.

**Correctness and contract checks**

- Read AGENTS.md, the wave header and W2-A contract, all of `03`, the required sections of `04`, `05`, and `09`, the candidate's contract decisions, handoff and SCR. Inspected the requested W2-A/integration changes without attributing unrelated changes to this candidate. Public signatures and field lists match the candidate contract; error variants are owned by W2-A. The diff changes aggregation, its tests, and its handoff/SCR; no dependencies, shared manifests, core contracts, or shared module declarations changed.
- Steps A–F implement maximum observed `nlink`, last-seen stat attributes, multiset instance counts, conservative completeness, pairwise parent-lifting LCA, single-found treatment, cumulative union stamps, additive rollup, and byte-sorted preorder IDs. Reverse preorder is a valid iterative substitute for depth buckets. Union contributions are added after rollup and are not counted twice. Directory blocks and age buckets participate correctly.
- An independent brute-force probe passed **726 link cases**, enumerating zero-to-two instances in each of five directories with three link-count choices. It checked complete/incomplete groups, duplicates within one directory, root-spanning sets, LCAs, disk/apparent unions, entry/inode counts, mtimes, reversed arrival, and Gone records. A separate race case verified maximum `nlink=4`, found two, incomplete status, and last-seen disk 8192. Order independence for conflicting stat observations is necessarily limited by the normative last-seen rule.
- Recomputed the golden example by hand, then asserted independently against actual aggregator rows:

  | Directory | Calculation and resulting disk | Charged | Reclaimable | Shared | Files / inodes / dirs |
  |---|---|---:|---:|---:|---|
  | `/r/a` | 1,048,576 + 4,096 = 1,052,672 | 1,048,576 | 1,048,576 | 4,096 | 2 / 2 / 1 |
  | `/r/b` | 4,096 + 8,192 = 12,288 | 8,192 | 0 | 12,288 | 2 / 2 / 1 |
  | `/r/c` | shared inode 103 counted once = 12,288 | 12,288 | 12,288 | 0 | 3 / 2 / 3 |
  | `/r` | 1,048,576 + 4,096 + 8,192 + 12,288 = 1,073,152 | 1,073,152 | 1,064,960 | 8,192 | 7 / 5 / 6 |

- Checked invariants 1–6 against implementation and tests. Numerical invariants 1–5 agree on the exercised fixtures/reference cases. Invariant 6 fails for error ordering as reported. Checked arithmetic protects metric accumulation, stamps, depths and ID conversions; notable IDs are zero-based checked `u32`. Error totals have the separate contract gap above.
- Step G orders by `(disk desc, final directory ID asc, inode asc)`, selects the threshold/top-N union under the cap, and chooses the smallest final-directory/name representative. Bulk resolution picks the smallest name and leaves instance names absent. Supplied golden/property tests compare all row classes between walk and resolved bulk inputs. Selection correctness passed; storage needed to reach selection is the blocker.
- Child-before-parent arrival resolves iteratively. Missing parents return `AggError::Tree`; holes and Gone records are tolerated, OtherFs becomes a skip, and races are counted. Traversal, LCA, annotation lookup, path reconstruction and destruction have no tree-depth recursion: nodes own numeric child IDs, not child nodes. The depth-10,000 test passed.
- Aggregation performs no filesystem I/O and has no unsafe blocks, threads or queues. It cannot follow symlinks, read scanned contents, mutate metadata, or bypass the Gate. `O_NOATIME` and mount-boundary acquisition belong to the unchanged walker. Paths stay byte-based and time comes from `scan_start`. Errors use `thiserror`; no library printing or unexplained `unwrap`/`expect` was introduced. The path-building `expect` has the required infallibility explanation. Sink errors stop subsequent writes.

**Memory assessment**

Re-running the layout test measured `Node = 288`, `Option<Node> = 288`, `Totals = 128`, `LinkAcc = 128`, instance slot 32, and `FileRow = 72` bytes. At five million nodes, 288 bytes means **1,440,000,000 bytes (1.341 GiB)** versus 1,000,000,000 bytes at the 200-byte target: **419.6 MiB extra**, before names, child allocations and approximately 60 MB for three `u32` numbering vectors. This matters against the default 1.25 GiB scan budget, even independently of candidate retention. The actual candidate rejects this directory count, as finding 4 describes.

Possible reductions: use a compact child-edge array with per-node ranges instead of a 24-byte `Vec` header per node; derive subtree directory counts from preorder intervals; derive suitable own counters from child totals; separate union/stamp scratch and release it after preparation. Measure ingestion and preparation peaks, not only the final node layout. The target miss is documented honestly in the handoff; it is not treated as a separate fifth finding.

**Build and test evidence**

Used pinned Rust 1.98.0, a temporary copy of the host Cargo registry at `/tmp/tidemark-w2a-review-cargo`, and `CARGO_NET_OFFLINE=true`. Cargo commands also used `--offline` where supported. All checks below used the exact candidate archive.

| Check | Observed result |
|---|---|
| `tools/ci.sh` | Exit 0, 230.451 s including first compilation; source checks, fmt, Clippy with warnings denied, workspace tests passed. 186 tests passed, zero failed, five scale tests ignored. `cargo-deny` unavailable: explicitly skipped. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0, 38.727 s; compilation only. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0, 13.742 s. |
| W2-A ordinary integration suite | 12 passed, zero failed; two scale tests subsequently run explicitly. Layout unit test also passed. |
| 2M directories / 30M aggregated entries, counting sink | Exit 0, 8.118 s, peak 679,432 KiB (663.5 MiB). Scope limitation on candidates described in finding 1. |
| 500K bulk link sets / 1M instances | Exit 0, 1.111 s, peak 262,072 KiB (255.9 MiB). |
| Independent golden/error/cap, link-oracle and notable-memory probes | All completed; assertions reproduced the reported defects and confirmed the stated correct cases. |

The task's required golden/model, real-walker/du, randomized reference/order, bulk-equivalence, deep-tree and scale tests exist and assert meaningful outputs. The random suite uses 40 weighted cases up to the specified bounds, supplemented by an explicit 2,000-directory/20,000-inode/2,000-link-set case. The real fixture compares measured disk usage per directory and root find counts. Scale tests use `#[ignore = "scale"]`. The loop-XFS golden test is feature-gated, not ignored; its directory-block correction is measured. No W2-A test asserts another task remains a stub. Error-order and candidate-heavy scale omissions are covered by findings 1 and 3.

No privileged tests, mounts, LVM operations, VM connections, connections to database servers, or production-scale OOM experiments were run. Loop-XFS/Rocky runtime behavior remains **pending VM**. Dependency advisory/license audit remains unavailable. The later integrated checkout was not subjected to another full build; this report judges the named candidate, not subsequent fixes or other tasks. The 30M candidate memory figure is a structural lower bound supported by smaller measurements, not a claimed full-scale run.

Evidence files are under this attempt's output `library/`: `candidate-ci.log`, `candidate-privileged.log`, `candidate-doc.log`, `layout.log`, `scale_two_million_directories_thirty_million_entries.log`, `scale_half_million_bulk_link_sets.log`, `golden_and_errors_probe.rs` / `golden-and-errors.log`, `link_review_probe.rs` / `link_review_probe.log`, and `notable_memory_probe.rs` / `notable-memory-*.log`. This report and its receipt are evidence submissions; acceptance and integration remain external decisions.

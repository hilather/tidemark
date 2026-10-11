**CHANGES REQUESTED** — 0 BLOCKER, 2 SHOULD-FIX, 0 NIT.

Reviewed candidate `4078ec99a4724a01ff2d6ad4d273cc98adf7e029` against base
`a17327de45fc3007e49b7658b3bd8ce2c964fb32`, scope `candidate_diff`.
The review checkout began at `40671085d313dd7af58511781faedc39b8e883d4`.
Its W3-B implementation/test files and relevant reader/directory helpers match
the candidate; unrelated files contain later changes. Findings below concern
the exact candidate delta. An archive of that candidate was also used for
separate build checks.

## Findings

1. **SHOULD-FIX — Rejects a result that fits the crossing-sample cap.**
   `crates/tidemark-store/src/links.rs:234` (also the duplicated accounting at
   line 493). Reference: W3-B `crossing_sets` streamed selection and bounded
   retained output; AGENTS.md rule 10.

   A better incoming set is added to `sample_count`, and the cap is checked,
   before the displaced worst result is removed. Thus the cap applies to
   obsolete samples as well as the requested result. A SnapshotWriter probe
   with 1,000 depth-1 regions and 1,001 complete sets, each spanning all 1,000
   regions with increasing disk size by inode, demonstrates the error:
   `crossing_sets(..., 999)` succeeds and counts all 1,001 sets, while limit
   1,000 returns `crossing sample cap reached`. The latter's final 1,000 sets
   require exactly 1,000,000 samples, within the documented cap.

   Compute the prospective retained sample total after subtracting any
   displaced result, and evict/debit it before allocating the replacement.
   Apply this to both selection paths and add a replacement-at-cap regression.
   Evidence: `library/store-review-cap.log` and `store-review-probe.rs`.
   Receipt reference: `finding:w3b-sample-cap-before-eviction`.

2. **SHOULD-FIX — Large supported limits make streamed selection quadratic.**
   `crates/tidemark-store/src/links.rs:413` (same pattern at lines 251 and 509).
   Reference: W3-B scalable streamed queries/performance acceptance and
   `08-interfaces.md` links `--top`.

   Every admitted candidate sorts the whole retained vector. Filling a large
   `top`, or repeatedly replacing its worst member, therefore revisits that
   vector for each inode. Limits up to 1,000,000 are accepted. On the same
   synthetic snapshot of 5,000 complete two-instance sets with increasing
   sizes, the candidate and exact base implementation returned the same
   ordered largest-set inode IDs:

   | Requested top | Candidate | Base |
   |---|---:|---:|
   | 25 | 0.135 s | 0.780 s |
   | 5,000 | 27.205 s | 1.003 s |

   These are debug comparisons during concurrent workspace validation,
   excluding fixture construction; they are not release acceptance timings.
   They expose a regression that the default-top million-set test misses.
   Use a bounded heap for admission/replacement and sort once at completion,
   retaining `(disk descending, LCA path bytes ascending, inode ascending)`.
   Cover large limits and increasing-size input in a regression/performance
   check. Evidence: `library/store-review-perf.log`, `store-review-probe.rs`
   and the copied base implementation `store-review-base-links.rs`.
   Receipt reference: `finding:w3b-top-selection-quadratic`.

## Checked and found correct

- Public signatures, field lists, flags, schema and dependencies are unchanged
  by this delta. The private connection and checked-directory helper match
  the accepted contracts recorded in `13-contract-decisions.md` R2-4/R2-2.
- The two inode-ordered cursors merge correctly; no global vector of sets or
  instances is retained. Directory IDs use one reusable per-inode buffer
  capped at one million. Region, ancestor, pair and output collections have
  explicit caps. This is source inspection, not a process RSS measurement.
- An independent brute-force oracle passed 26,560 assertion groups over 16
  generated snapshots and 2,880 sets: root/subtree selection, nested and
  overlapping globs, relative depths, outside-region points, 3,346 repeated
  directory instances, 711 unmatched-region crossing cases, 783 non-UTF-8
  region matches, ordering, samples, totals, pinned unions, pairs and LCAs.
  Source and results: `library/w3b_oracle.rs`, `.log`, `-notes.txt`.
- The normative worked example gives three sets, five instances and 24,576
  bytes; inode 100 crosses a/b with 4,096 bytes, inode 102 is incomplete and
  pins 8,192 bytes in both `/r/b` and `/r`, and inode 103 remains within c.
  Largest order is 103 (`/r/c`), 102 (`/r/b`), 100 (`/r`). Existing golden,
  scanned hardlink-web, region-property and renderer tests exercise these
  contracts without assuming host allocation sizes.
- Bulk `--show-paths` now passes snapshot root/device and expected directory
  inode to `read_dir_entries_checked`. Opened handles are checked before
  enumeration; opens use no-follow/no-atime with EPERM fallback and gated
  metadata I/O. Missing/replaced directories become `(gone)`; other errors
  propagate. The new replacement-inode test meaningfully exercises the fix.
- The unchanged symlink traversal is iterative and bounded, does not descend
  through links, and performs one explicit target stat when requested. An
  additional CLI probe passed `/proc`, a self-loop, an outside directory with
  a sentinel, a FIFO, absolute/relative links, a 4,000-byte target, 1,000 `..`
  components and invalid UTF-8. Both modes counted exactly ten links;
  path/target base64 round-tripped. Artifacts: `library/safety-probe.py` and
  `safety-probe.json`. Text/JsonDoc snapshots and CLI surface/exit-code tests
  were also exercised by checkout CI.
- The delta adds no unsafe code, regular-file content reads, filesystem
  mutation, ungated live enumeration, recursive traversal or wall-clock
  lookup inside store query logic. SQL uses bound parameters on the
  immutable read-only snapshot connection.

## Validation

All Cargo checks use pinned Rust 1.98.0, `CARGO_NET_OFFLINE=true` and a private
copy of the host registry cache; direct Cargo commands additionally use
`--offline`. No network dependency fetch was attempted.

| Check | Review checkout | Exact candidate |
|---|---|---|
| `tools/ci.sh` | Pass: source policy, fmt, Clippy, 472 tests/doctests; 12 ignored scale tests | Exit 1: Clippy cannot compile unchanged W1-B tests |
| `cargo build --workspace --features privileged --tests` | Pass, compile only | Exit 101: unchanged W1-B/W2-A tests fail compilation |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Pass | Pass |
| Three crates' `--test w3b` suites | Passed within CI | Pass: 14 tests; 2 scale tests ignored |
| Release full-CLI million-set test, explicitly `--ignored --nocapture` | Not run | Pass: 3.847812666 s, below 10 s |

Exact-candidate failures outside this diff: `crates/tidemark-fs/tests/w1b/main.rs:15` lacks `WalkConfig.max_record_bytes`; lines 599 and 726 match `WalkError::Limit(String)` with string literals. The privileged build additionally finds the same missing field in `crates/tidemark-scan/tests/w2a/main.rs:553`. These are build limitations, not findings against W3-B.

The exact-candidate scale command was
`cargo test --offline --release -p tidemark-cli --test w3b million_set_cli_timing -- --ignored --nocapture`.
It counted all one million crossing sets at boundary depth 2 with the default
limit of 25. The 3.85-second interval includes CLI launch, query, rendering,
output collection and JSON decoding; fixture setup is excluded. This is a
local shared-host observation, not a VM result or a general performance guarantee.
The separate million-set library scale test was not explicitly rerun.

Cargo-deny is not installed; checkout CI explicitly skipped its audit.
The two ignored W3-B scale tests are distinct from privileged tests.
The 10,000-set comparative debug probe was stopped after the completed
5,000-set comparison established the regression; no larger timing is claimed.
Full command logs and exit files are in the canonical output `library/` for
attempt `attempt-2fad877e5fb7bd9eec017ab1da561e7f8d0b2fa440f4e772dc7bba7a21218d72`.
`review-provenance.json` records source equality. An initial shared-target CI
invocation reused a helper with the checkout path embedded in it; that run
was discarded and interrupted. The exact-candidate logs use a fresh target
directory, as recorded in `exact-validation-note.txt`.

## Limits of this review

No privileged test was executed and no VM was contacted. Cross-filesystem
mount and same-device bind-mount runtime behavior remains pending VM; the
privileged tests are feature-gated, not ignored. Forced O_NOATIME EPERM
fallback was not exercised. No whole-process memory/RSS or deep-incomplete-set
performance claim is made.

Two unchanged symlink limitations are outside this candidate-diff verdict:
same-filesystem secondary symlink escapes retain lexical `InsideRoot`
classification (also reproduced by the probe), and the allocating rustix
`readlinkat` helper can retry internally for long targets under one Gate
charge. Thus per-syscall charging for that preexisting long-target path is
not established. The new symlink-replacement regression uses an empty
outside directory and alone would be weak evidence against descent; helper
inspection and the separate sentinel probe supplement it.

The report is review evidence, not an integration or release decision.

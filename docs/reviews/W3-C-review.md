**APPROVE — 0 BLOCKER, 0 SHOULD-FIX, 0 NIT in the candidate diff.**

Reviewed `1040b2b644127b979b73e70bbf0473024b50940f` against `37792b7ebb233d06ca7b7ae0d9eafd0501696230`, under the supplied `candidate_diff` scope. This is a review recommendation, not an integration decision or VM-gate clearance.

The supplied checkout is `aa08b8e4a25a3cb94a69d8c407eaeb0ca17ab731`. Its four changed W3-C Rust files, store/core crates, CLI source, and dependency manifests match the candidate. Builds ran in this checkout, which additionally contains unrelated scanner/test and plan changes; they are not represented as whole-candidate-tree validation. `library/scope.json` records the matching file blobs.

No introduced correctness or contract defect was verified. The new SQL access uses the accepted internal accessor from `13-contract-decisions.md` R2-4. No public signature, schema, dependency, CLI flag, or shared module declaration changes appear in the diff.

**Checks and evidence**

- Read AGENTS.md, the Wave 3 header and W3-C contract, storage §10, export/wide interfaces, contract decisions, W3-C handoffs/SCR, and the normative link example. Compared the emitted representation with the [upstream ncdu JSON format](https://dev.yorhel.nl/ncdu/jsonfmt): root array, directory/file infoblocks, inherited device, size fields, and raw-byte name convention agree. Invalid UTF-8 in raw mode is intentional; strict JSON requires lossy mode.
- The directory query walks preorder ids with an explicit stack, capped at 65,536 open arrays. Notable queries retain one borrowed row and the last paging key; the 256-row LIMIT bounds SQLite sorting storage. The `(name, ino, id)` key preserves deterministic byte order and page boundaries. No tree-sized Rust collection or recursive traversal was introduced.
- Named single-link entries are subtracted with checked arithmetic; unnamed and multilink entries remain aggregated. Directory blocks are emitted separately, `asize` and `dsize` retain their meanings, and the final disk sum is checked against the exported root's `t_charged`.
- A separate public-store probe passed 522 notable files across three SQL pages, every permitted control byte, invalid UTF-8 sequences, Unicode, differing disk/apparent sizes and anonymous remainders. A 5,000-directory chain passed independent iterative JSON parsing at full depth and limits 0, 17 and 4,999; every variant retained disk/apparent totals. Hard-link probes retained additive disk totals for whole roots and subroots. Sources, commands, JSON and logs are in `library/hostile-probe-source/`, `hostile-probe-direct/`, `hostile-probe-direct.log` and `hostile-probe-json-parse.log`.
- The CLI hostile-fixture probe passed for quotes, backslashes, controls, invalid UTF-8, a hostile root path and a 255-byte filename. Lossy output parsed, raw bytes survived, individual sizes matched measured metadata, and summed `dsize` matched the snapshot. Closed stdout returned exit 1 with `Broken pipe`, without panic. See `library/cli_hostile_pipe_probe.py` and `.log`.
- Publication stages a private file beside the destination, flushes and syncs it before `RENAME_NOREPLACE`. Existing destinations, snapshot symlinks, and aliases into state_dir were refused without changing the snapshot. The added regressions cover failure after export starts, staging cleanup, and a destination appearing before publication. No production `unsafe`, panic/unwrap, scanned-file content read, live resolution, or ungated scan I/O was added.
- The unchanged wide path calls `wide_dirs` and renders entries, `t_disk`, byte-safe paths and the app-owner/btree/ls hint. Independent CLI checks passed descending entry order, name ties, zero/excessive limits and non-UTF-8 JSON paths.

**Validation results**

Rust 1.98.0; copied host Cargo registry; `CARGO_NET_OFFLINE=true`; explicit `--offline` where accepted. No privileged tests or VM commands were executed.

| Check | Result |
|---|---|
| `tools/ci.sh` | Exit 0; source checks (135 owner tags), fmt, Clippy, workspace tests and doctests passed: 472 passed, 0 failed, 12 ignored |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 |
| `cargo test --offline -p tidemark-store --test w3c million_directory_export_rss_below_64_mib -- --ignored --nocapture` | Exit 0; 1,000,000 directories, sampled RSS delta 0 bytes; export 217.78 s, test 251.51 s |
| Independent hostile/deep store probe and iterative JSON parser | Exit 0; direct rustc build against completed default-feature libraries |
| Independent CLI export/closed-pipe/wide probe | Exit 0 |

CI passed the new atomic-publication tests, scanned link/non-UTF-8 fixtures, nested/subroot/depth cases and the 4,097-notable-file regression. Its ordinary ignored tests include the scale test run separately above. Cargo-deny was not installed and CI skipped it; dependency advisory/license checking remains unverified. Logs and exit files are `library/{ci,privileged,doc,scale}.{log,exit}`; totals are recorded in `library/validation-summary.json`.

**Limits and follow-up evidence**

The new paging query at `crates/tidemark-store/src/export_ncdu.rs:47` repeatedly scans a directory because the frozen schema has no name-order index. An independent in-memory SQLite probe of this query and index layout used approximately 665 million VM operations for 100,000 notable files and 2.66 billion for 200,000 (28.16 s and 89.78 s respectively on this shared host). Doubling input quadruples SQL work. These are SQL-probe measurements, not whole-export or target-VM timings. The handoff explicitly documents this memory/time tradeoff and W3-C specifies no export-throughput threshold, so it is recorded as a scalability limitation rather than a counted finding. A separately approved ordering index or bounded disk-backed sort would merit follow-up. See `library/export_paging_probe.py` and `.log`.

The scale test samples RSS per MiB emitted and after export, against a baseline after fixture construction. Its delta is not an absolute-memory measurement or a continuous high-water bound. Actual ncdu import on Rocky/XFS remains pending VM. Publication does not sync the destination directory, so power-loss durability of its new name was not established. No concurrent ancestor-replacement or crash-injection test was run. The review does not certify unrelated or pre-existing code outside the specified diff.

The store probe also exposed two inherited behaviors, excluded from the candidate finding count after comparison with the base: at `export_ncdu.rs:108`, adding `own_symlinks` to `own_files` makes the synthetic label report four remaining entries where there are three, because symlinks are already a subset of `own_files`; at `export_ncdu.rs:183`, depth truncation uses union-based `t_apparent`, so a subroot's apparent sum changed from 32 to 532 when a 500-byte hard link was charged outside that subroot (disk remained 28). These observations limit any interpretation of this approval as a review of the entire pre-existing exporter.

Supporting artifacts are under `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-7950dba800d6bfa6049fd97874c131c2173e946792cd118f78bdf6b2d7d30211/library/`.

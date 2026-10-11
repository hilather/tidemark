**CHANGES REQUESTED** — 1 BLOCKER, 3 SHOULD-FIX, 0 NIT.

Reviewed candidate `c48f5b292b0d2b260af14789ee17d96591e4f0e9` against base `a89ea75ce1f16aa2e3148a7bcb50a4cb48b622e5`. All W1-A source and test files are byte-identical in the supplied checkout, `434b69dacb85e4c54e6d9c1424f70cdb59f0a86c`. Workspace checks below ran on that checkout; later changes from other tasks are distinguished from the candidate. This is review evidence, not an integration decision.

1. **BLOCKER — `crates/tidemark-fs/src/mount.rs:70`: one valid nsfs record disables all mount discovery.** Reference: W1-A mountinfo details; `04-scanner.md` §2.2; `03-link-semantics.md` §5; pending `docs/handoff/scr-W1-A/SCR-001.md`. Receipt reference: `finding:w1a-nsfs-mount-table`.

   A table containing an ordinary XFS record followed by `2 1 0:5 net:[4026533000] /run/netns/test rw - nsfs nsfs rw` returns an error, because its object root cannot inhabit `RawPath`. `read_mountinfo` propagates that error for the entire table. A record of this form also exists in this review environment. The executable consequently reports mount type, device, XFS and fanotify discovery as unknown for every root, including unrelated filesystem roots. The committed regression currently asserts rejection rather than usable mount discovery.

   Resolve the existing SCR-001 with the coordinator, then implement and test the approved byte-preserving root representation. Preserve the nsfs mount target in boundary queries and retain the other records. Do not silently discard the record or fabricate an absolute root. The candidate correctly disclosed the frozen-contract conflict; that disclosure does not remove this integration blocker. Evidence: `mount-canon-probe.log` and the executable output in `candidate-surface-probe.log`.

2. **SHOULD-FIX — `crates/tidemark-cli/src/cmd/probe.rs:24` and `crates/tidemark-fs/src/probe.rs:262`: lexical normalization precedes realpath.** Reference: `13-contract-decisions.md` SCR-004, which explicitly requires canonicalizing live query paths before constructing lexically normalized `RawPath`; `03-link-semantics.md` §5. Receipt reference: `finding:w1a-probe-path-resolution`.

   Both CLI and configured roots lose `..` before `canon::canonicalize` runs. With `/tmp/base/proc_link -> /proc/sys/kernel`, requesting `/tmp/base/proc_link/..` reports `/tmp/base`, although realpath resolves `/proc/sys`. The executable reported the measured capacity of the filesystem containing the temporary fixture instead of the requested proc filesystem's capacity. Likewise, `/tmp/base/missing/../existing` reports a successful existing root with known capacity and no canonicalization error, although strict realpath returns ENOENT. Six executable cases reproduced both failures through both input routes. The standalone `canon::canonicalize` function handles these inputs correctly; its callers destroy the original path first.

   Canonicalize each original `PathBuf` before converting it to `RawPath`. Carry an original canonicalization failure into the root's unknown facts instead of retrying a normalized fallback as if it resolved successfully. Keep the frozen public signatures; use private collection helpers or command-side composition as needed. Add CLI and configured-root regressions for symlink/parent traversal, crossing to another filesystem, and missing intermediate components. Evidence: `probe-root-paths.py` and `probe-root-paths.jsonl`.

3. **SHOULD-FIX — `crates/tidemark-fs/src/mount.rs:98`: UTF-8 mount sources bypass safe display escaping.** Reference: AGENTS.md rule 7; `13-contract-decisions.md` SCR-004 control/backslash display rules. Receipt reference: `finding:w1a-source-control-display`.

   The valid escaped source `/dev/a\012spoof` is decoded to a String containing a literal newline. The UTF-8 success branch returns it unchanged, and `crates/tidemark-cli/src/render/probe.rs:79` writes the fact verbatim. A probe through the actual parser and renderer produced `  ✓ source: /dev/a`, followed by a separate unmarked `spoof` line. Other UTF-8 control characters can reach the terminal by the same route. Non-UTF-8 absolute sources already take the escaped-display branch, making display inconsistent.

   Apply the prescribed control-character and literal-backslash escaping to every displayed mount source, including valid UTF-8, while preserving printable Unicode. Add parser-to-renderer regression cases for newline, escape and backslash sources. Evidence: `mount-canon-probe.rs` and `mount-canon-probe.log`.

4. **SHOULD-FIX — `crates/tidemark-fs/tests/w1a/data/README.md:3`: required captured EL8 parser fixtures are absent.** Reference: W1-A Tests explicitly requires 3–4 real captured EL8 mountinfo samples. Receipt reference: `finding:w1a-el8-fixtures`.

   All four supplied files are explicitly synthetic. They cover useful edge cases, but neither their passing tests nor the handoff establish parsing of the required real inputs. Supply 3–4 captured EL8 samples covering bind mounts, escaped spaces and `/dev/mapper/*` sources, document their provenance, and run the parser assertions against them. Retain the synthetic adversarial cases. Capture collection can remain pending access to an authorized VM; it must remain an unmet acceptance input until supplied. No VM was accessed during this review.

Checked and found correct:

- The candidate changes no manifests, lockfile, core contracts or other task implementation. Its renderer addition is permitted by W0-C's renderer-extension rule; the only shared-file change is the documented `render/mod.rs` declaration. Required mount/blockdev/canon/probe signatures and the frozen `MountInfo`/`ProbeReport` field lists remain intact.
- Mount optional fields, octal/raw-byte path decoding, component-prefix boundaries, longest match, later equal-prefix mounts, sorted submounts and same-device filtering passed existing and supplemental probes. Standalone canonicalization preserves non-UTF-8 names, resolves relative paths and symlink/parent traversal, and returns ENOENT/ELOOP for missing paths/loops.
- Device traversal promotes partitions, resolves dm/md topology, deduplicates and sorts leaves, collects md levels, and bounds graph work. Supplemental probes using the candidate's blockdev source with the real core/libc dependencies passed cycle, missing-device/attribute, oversized graph/stat and sample-cap cases. Disk-stat tests cover 11/15/17 fields; sampling preserves order and duplicates and fails the whole call for an unreadable or malformed device.
- PostgreSQL is composed in the CLI, not imported by tidemark-fs. Known recommendation rules are deterministic and unit-tested. The JSON renderer snapshot, non-UTF-8 path encoding and explained section-level unknowns passed. XFS directory opens use NOATIME with EPERM fallback. No unsafe block, scanner traversal or system-mutating command was added to production W1-A code; its discovery reads are distinct from scan-path Gate I/O.
- All four W1-A privileged tests exist, are gated by `feature = "privileged"` rather than ignored, and compile: loop-XFS geometry, loop-LVM PV resolution, real bind mount queries and md members. Their geometry/device expectations use external measurements/oracles.

Validation and limits:

Used pinned Rust 1.98.0, `CARGO_HOME=/tmp/tidemark-w1a-review-cargo` copied from the host cache, `CARGO_NET_OFFLINE=true`, and `CARGO_TARGET_DIR=/tmp/tidemark-w1a-review-target`. Artifacts are in this attempt's output `library/` directory:
`/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-848733697f7356d6081aba657967938d0162ea083da272593785a7706725b7d3/library/`.

| Check | Observed result | Artifact |
|---|---|---|
| `tools/ci.sh` with the pinned toolchain on PATH | Exit 0: source checks, fmt, Clippy with `-D warnings`, and workspace tests pass; 148 tests/doc tests passed, 2 unrelated scale tests ignored | `ci.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compiled only | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | `rustdoc.log` |
| Supplemental mount/canon/render probes | Exit 0; asserted normal behavior and reproduced findings 1 and 3 | `mount-canon-probe.log` |
| Supplemental sysfs/stat probes | Exit 0 | `blockdev-probes.log` |
| Six CLI/config path probes | Commands exit 0 but report incorrect roots, as finding 2 describes | `probe-root-paths.jsonl` |
| Candidate's exact `binaries_and_bash_completions` test replayed against the built W1-A implementation | Exit 101: candidate expects probe exit 1, observes 0 | `candidate-surface-probe.log` |

The last failure is the disclosed out-of-scope stale test in SCR-002, not a new W1-A finding. The supplied checkout already removes that assertion, explaining why its complete CI run passes. A complete rebuild of the historical candidate workspace was not performed; the exact candidate diff and unchanged W1-A blobs were reviewed, and the candidate-only assertion was separately replayed.

Cargo-deny is unavailable and CI explicitly skipped it; no dependency advisory/license audit is claimed. No privileged tests, mounts, LVM changes or VM commands ran. Actual Rocky XFS/ioctl behavior, loop-LVM/md/bind acceptance and real EL8 captures remain unverified. LVM, fanotify and PostgreSQL implementation integration remains future work; specifically, the current LVM section is hardcoded unavailable rather than calling its stub, and W4-A explicitly owns that section. The report does not claim end-to-end behavior for those later collectors or exhaustive handling of every host-metadata failure.

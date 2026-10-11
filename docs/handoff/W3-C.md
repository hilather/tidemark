# Handoff — W3-C: ncdu export and wide directories

## Needs from coordinator

- Resolve [SCR-001](scr-W3-C/SCR-001.md) and provide an internal Snapshot connection accessor or streaming per-directory file cursor. The current exporter rejects a subtree with more than 4096 notable files. This is an explicit interim limit; full notable-file streaming is not done.

- **Branch:** hp-703c38024e27de12a19b6a789c6b239492fbafe59b8251511eadbc597f6aa596
- **Agent/session:** attempt-703c38024e27de12a19b6a789c6b239492fbafe59b8251511eadbc597f6aa596
- **Date:** 2026-10-11
- **Status:** implemented core; blocked full notable-file streaming on shared reader access. Evidence candidate, awaiting independent review.

## What was built

The store exporter emits ncdu `[1,2,metadata,root]` using preorder directory lookups and an explicit stack of subtree ends; no directory tree or depth recursion is used. It writes directory own sizes, named single-link notable files, and one aggregate of remaining own charges, preserving raw name bytes or replacing invalid UTF-8 on request. Depth-limited descendants become an aggregate to preserve the root charged total. Counts/sizes use checked subtraction/addition, the final total is checked against root `t_charged`, and I/O errors propagate.

The export command selects an immutable snapshot, streams to buffered stdout or an exclusively created `0600` file, refuses every existing destination, and rejects canonical parent paths inside state_dir (including symlink aliases). Wide selects the latest complete snapshot, calls `wide_dirs`, and renders entries, subtree disk bytes and canonical paths, with the app-owner/btree/ls hint in text and schema-wrapped byte-safe JSON. Public signatures/argument field lists are unchanged; ExportStats was owner-defined.

## Shared-file edits

None. No manifests, lockfiles, shared module declarations, frozen core types, plan files or memory projections changed. All dependencies were already attached.

## Deviations from the spec

| Spec reference | Implementation / assumption | Reason | SCR |
|---|---|---|---|
| W3-C streaming notable files | One bounded list of at most 4097 public-query FileHits; reject >4096, sort retained single-link named hits by dir/name/ino | Snapshot hides connection and exposes only collecting subtree query; fail rather than allocate unbounded data | SCR-001 pending |
| W3-C explicit stack | At most 65536 open arrays; deeper inputs fail | Documented independent depth cap | No public contract change |
| `--max-depth` | Relative to export root; cut descendants emitted as `[depth-limited descendants]` | Plan does not specify truncation accounting; retains additive disk total | No |
| Bulk notable names absent | Leave unnamed entries in aggregate, no live resolution | Metadata-only snapshot export cannot invent names | No |
| `export --json` | Export remains intrinsic ncdu JSON, without Tidemark envelope | Wrapping would invalidate ncdu's frozen format | No; assumption for otherwise conflicting global JSON rule |
| Output overwrite | Refuse all existing files, not only snapshots | Safely includes snapshots outside state_dir, hard links and symlinks | No |
| Build environment | Pinned host toolchain binary path, copied writable cargo cache, CARGO_NET_OFFLINE=true | Cargo absent from PATH; default rustup home read-only; crates.io unavailable | No |

Format was implemented from the plan's description, not the unreachable ncdu format page. Loading the file in real ncdu is pending VM.

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Export loads in ncdu 1.x/2.x VM | `vm_ncdu_import_reexport`, feature privileged, optional ncdu package | pending VM |
| Metadata note says charged/additive | `nested_arrays_subroot_depth_and_charged_totals` checks note | locally passed |
| Scanned link fixture dsize sum equals t_charged | `scanned_link_fixture_additive_export`, measured scanned totals | locally passed |
| Non-UTF-8 fixture correct raw/lossy escaping | `scanned_nonutf8_preserved_or_lossy_and_escaped` | locally passed |
| Streaming 1M directories RSS delta <64 MiB | `million_directory_export_rss_below_64_mib`, ignore=scale | locally passed; measurement below |
| Private output and state protection | `export_mode_overwrite_state_protection_and_wide_json` | locally passed final workspace tests |
| Wide columns and hint | Same CLI test covers text/JSON, limit and hint | locally passed final workspace tests |

## Tests

Added store tests: `nested_arrays_subroot_depth_and_charged_totals`, `notable_cap_fails_before_writing_and_io_errors_propagate`, `million_directory_export_rss_below_64_mib` (scale). Synthetic rows intentionally use abstract byte values; filesystem fixture values are measured through scans.

Added CLI tests: `scanned_link_fixture_additive_export`, `scanned_nonutf8_preserved_or_lossy_and_escaped`, `export_mode_overwrite_state_protection_and_wide_json`. Added privileged test: `vm_ncdu_import_reexport` — pending VM; only feature-gated, not ignored, returns with a diagnostic if ncdu is missing as the plan permits. No privileged host commands were run. No tests ignored pending another task; only the scale test is normally ignored.

## Measurements

`cargo test --offline -p tidemark-store --test w3c million_directory_export_rss_below_64_mib -- --ignored --nocapture` passed. Final scale-peak.log: 1,000,000 directories, RSS delta **0 bytes**, export elapsed **113.680 s**, total test elapsed 151.13 s. RSS is sampled from /proc/self/status every MiB of emitted output and after export; it is a sampled peak, not a continuous process high-water mark. This is a flat synthetic tree without notable files, debug build, Linux 7.2.5-3-omarchy, 12 available CPUs, concurrent workspace validation load. Earlier end-only measurement was also 0 bytes and 86.059 s; final sampled measurement is the acceptance evidence. These numbers do not establish target VM performance.

## Local validation

Final checks on the delivered source all returned exit 0. Logs and exit files are in the attempt output `library/`; CARGO_NET_OFFLINE=true and the copied cache apply to all Cargo commands.

| Command | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | fmt.log / fmt.exit |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | clippy.log / clippy.exit |
| `cargo test --offline --workspace` | exit 0; 302 passed, 0 failed, 7 ignored (includes doctests) | test.log / test.exit |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only | privileged.log / privileged.exit |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | doc.log / doc.exit |
| `tools/ci.sh` (`cargo xtask ci`) | exit 0; 129 owner tags; forbidden-symbol scan passed | ci.log / ci.exit |
| Sampled-RSS scale test | exit 0 | scale-peak.log / scale-peak.exit |

CI reports cargo-deny unavailable and skips its check: advisory/license validation remains pending an equipped host. The initial check launcher had a log filename error for tools/ci.sh; the corrected final launcher ran it successfully. Earlier logs prefixed cargo- are superseded by the final plain-name logs above. No failures in unrelated files blocked validation.

## Known gaps and follow-ups

- Full notable-file streaming above the interim cap needs coordinator accessor approval and a follow-up implementation. Million-directory width is supported when notable counts stay under the cap.
- Directory retrieval uses one indexed lookup per preorder id rather than one SQL cursor. Benchmark evidence covers this implementation, not a promised future cursor.
- ncdu import/export and target Rocky/XFS behavior remain pending VM.
- An output I/O failure after exclusive creation can leave a partial export file; callers receive an error. No atomic output replacement is promised by the plan.
- Do not interpret local checks or submission as verified task success or VM gate clearance.

## Notes for the reviewer

Names escape all JSON control bytes, quotes and backslashes while leaving invalid UTF-8 bytes raw unless lossy encoding is chosen. Multi-link notable files never get separate entries, preventing double charging; absent bulk names remain in the aggregate. Root info has the full canonical export path and device; child directories have component names. Preorder is scanner-established name-byte order. The iterator skips bounded notable entries under omitted descendants. Stdout carries no human header or success summary, so it remains importable ncdu data.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits
- [x] Public items documented; owner headers present
- [x] Privileged ncdu test written and listed as pending VM
- [x] No unexplained TODOs, production unwraps or expects
- [ ] Full notable-file streaming (SCR-001)
- [ ] VM acceptance evidenced
- [x] Final available CI results recorded above (cargo-deny unavailable)

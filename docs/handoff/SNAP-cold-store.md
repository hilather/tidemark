# Handoff — SNAP-cold-store

## Needs from coordinator

- Independently review and run the uid 1000, umask 022, socket-enabled and GitHub ubuntu-24.04 gates. This worker provides evidence, not verified integration success.
- SNAP-cold-daemon must pass its configured retention policy through `Snapshot::open_with_space_guard` (or adopt equivalent policy wiring), call `point_to_cold` after compression, then unlink the source and fsync the directory. The default open applies the existing default retention policy.
- Privileged execution remains pending VM; this worker builds privileged tests only. Run cargo-deny on an equipped host if the local wrapper skips it.

## What changed

Added a bounded 1 MiB JSON cold header containing user_version, all original meta table strings, and the serde root DirRow. The binary prefix preserves the specified magic, uncompressed byte length and u32 JSON length. Metadata and root counters are validated before catalog reconstruction. Compression streams through a 128 KiB buffer, enables the zstd checksum, checks cancellation, writes an exclusive 0600 temporary, fsyncs it, renames NOREPLACE, and fsyncs the directory. It leaves catalog updates and hot-source removal to callers.

Cold opens guard the exact uncompressed length before allocating an inflate leaf, stream into a private `.inflate/<scan>.<pid>.<n>.db`, and retain a flock until the SQLite connection closes and the leaf is removed. The catalog-facing filename remains the cold file. An injected guard allows configured policy and deterministic refusal tests. Existing v1 query methods are unchanged.

Catalog rebuilding reads cold headers without inflating, prefers the cold form when both exist, and reconstructs root totals. Cleanup removes cold temporaries, unlocked inflate leaves, and hot duplicates only after full streaming validation of the cold frame; it updates the catalog before unlinking the hot copy. Subdirectory and cold-leaf mount IDs are checked, including same-device bind mounts. `cold_plan` excludes retention deletions, each root's newest complete snapshot, the protected recent set, incomplete scans and already cold files.

## Shared-file edits

Only the `pub mod cold;` declaration directly after `pub mod catalog;` in lib.rs, with a blank separator so rustfmt preserves that required position. No dependencies or manifest changes.

## Assumptions and bounds

- The existing store refusal variant is `StoreError::Space`, not a variant literally named NoSpace. Inflate refusal uses that existing error without changing the error contract.
- `recent_uncompressed` protects at least that many newest complete scans, and never reduces `policy.recent`. All complete scans count in this ordering, including already cold scans.
- Inflate cleanup uses per-leaf flock ownership rather than PID existence or age. PID reuse cannot cause an active locked inode to be swept. A directory flock coordinates creation with the sweeper until the leaf lock is established. No wall clock is used.
- The decoder window is capped at 128 MiB. Output is limited to the declared length plus one byte to detect overflow; short frames, trailing data and length mismatches fail. Header metadata uses the same validation rules as the v1 reader, duplicated privately to keep reader changes confined to opening and lifetime fields.
- Cleanup fully decodes into a sink before deleting a duplicate hot file. Catalog rebuilding itself remains header-only. Published snapshot sources are immutable, as required by the existing writer contract.
- Reader v2 query support remains outside this task; no v2 files exist yet. The cold header accepts versions 1 and 2 for subsequent reader integration.

## Tests and validation

Final command outcomes and counts are recorded below after validation. Full logs are in this attempt's output library.

New integration cases cover all reader methods, byte-identical inflate, drop cleanup, injected space refusal before leaf creation, malformed magic/header bounds, truncated frames, shorter/longer declared lengths, trailing bytes, exclusive destination collisions, cancellation temporaries, cold-only rebuild totals, live versus abandoned inflate locks, corrupt cold duplicates preserving the hot source, default space policy, modes, symlinks and hard links, and recent/newest/daily/weekly/status/cold/deleted planning cases.

Before: the baseline has no cold module, compression/inflate API or cold file open path; the added acceptance tests could not compile against that API surface. No baseline test failure is claimed. During development, the first six-test run had one calendar fixture failure: a “recent” row at midnight minus one second was also the previous day's representative. Moving the fake time within the day corrected the fixture, and all six then passed. Clippy initially caught two redundant conversions and an item after a test module; these were corrected.

After: see final validation results below. No real-time test assertions, sockets, privileged commands or host configuration changes were introduced.

Privileged test added: `cold_open_and_cleanup_refuse_same_device_inflate_bind_mount` creates a loop XFS fixture in a VM, bind-mounts an outside directory over `.inflate`, and checks opening refuses and cleanup preserves its sentinel. Compiled with the workspace privileged build; **pending VM**, never executed here.

## Final validation results

All Cargo commands used the pinned Rust 1.98.0 toolchain, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`; long commands ran under `timeout 1500`. The worker is uid 0 with sockets restricted.

| Command | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0, including final module ordering |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --workspace --no-fail-fast` | Exit 0; summed completed harnesses: 1,054 passed, 0 failed, 14 ignored (includes child harnesses) |
| `cargo test -p tidemark-store --test cold` | Exit 0; 8 passed, 0 failed |
| `cargo build --workspace --features privileged --tests` | Exit 0; VM-only cold bind-mount case compiled, never run |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Exit 0 |
| `git diff --check` | Exit 0 |

Before/after output for the development fixture correction:

```text
before: cold_selection... FAILED; left: ["weekly"], right: ["daily", "weekly"]
after:  test result: ok. 8 passed; 0 failed; 0 ignored
```

The separate CI wrapper's final outcome is appended below. Independent coordinator verification, uid 1000/socket-enabled execution, GitHub and VM execution remain outside this worker's evidence.

`tools/ci.sh`: exit 0; `source checks: 152 owner tags; forbidden-symbol scan passed`; summed completed test harnesses including its two strict-ancestry tests: **1,056 passed, 0 failed, 14 ignored**. The final privileged-feature build passed. Wrapper output ended:

```text
warning: cargo-deny is not installed; skipping cargo deny check
ci: all available checks passed
```

No dependencies were added by this task. The equipped cargo-deny gate remains a coordinator need. All ten changed repository paths are in the assigned scope; `git diff --check` is clean.

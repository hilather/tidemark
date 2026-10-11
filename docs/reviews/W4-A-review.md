**CHANGES REQUESTED** — 0 BLOCKER, 1 SHOULD-FIX, 1 NIT.

Reviewed candidate `9d2f5331f63c626afd9a63fd277100cc6effdaa5` against `df21b52c1158a90925e5345132888d1d8d0de332`, limited to the candidate diff. All eleven changed files are identical in the supplied checkout at `a17327de45fc3007e49b7658b3bd8ce2c964fb32`; builds below used that checkout, including later unrelated integration changes. Read the task/read-first documents, contract decisions, W4-A handoff and candidate diff. There is no `docs/handoff/scr-W4-A/` directory.

**Finding F1 — SHOULD-FIX: complete the mandatory privileged LVM scenarios.**

Reference: `finding:w4a-privileged-scenarios`.

Locations: `crates/tidemark-fs/tests/w4a/lvm.rs:195`, `:132`, `:167`. Contract: W4-A Tests, which explicitly references `09-testing.md` §5; AGENTS.md Tests requires writing privileged tests even when execution is pending VM.

The three required test names exist, but their bodies do not cover all prescribed scenarios:

- `lvm_absent_is_na` only invokes the private command helper with `/missing/tidemark-vgs` and checks `NotInstalled`. This is an unprivileged missing-executable test, already covered elsewhere. It never creates a non-LVM loop filesystem, enables collection with installed tools, or asserts a rendered `n/a` result. It would still pass if existing LVM elsewhere on a host caused a non-LVM mount to be misclassified or its report to fail. Marking this test pending VM does not supply the missing scenario.
- `lvm_maps_mounts_to_lvs` creates only a linear LV (`thin_pool: None`) and checks mount/VG membership, not the exact LV identity or thin-pool mapping required for the combined linear/thin fixture.
- `lvm_thin_pool_usage_tracks_writes` checks only that data percentage increases. The independent `lvs` comparison and above-threshold `thin_pool_pct` alert assertion specified in §5 are absent.

Recommended change: retain the missing-tool assertion as an unprivileged test; implement the non-LVM case with `LoopXfs`, `lvm.enabled = true`, installed reporting tools, and assertions that the mount remains unmapped and the report renders `n/a` without error. Extend the mapping fixture with both linear and thin LVs and assert the exact LV/pool identities. Compare thin-pool usage to the reporting oracle and add the specified alert assertion in an appropriate integration test. Keep destructive setup behind `feature = "privileged"`, compile these tests locally, and leave their execution pending VM. This finding concerns missing test implementation, not the unavailable VM run.

**Finding F2 — NIT: keep renderer helpers inside the CLI crate.**

Reference: `finding:w4a-render-visibility`.

Location: `crates/tidemark-cli/src/render/lvm.rs:9` (also `:23`, `:38`, `:51`). The review's scope/contracts criterion disallows additional public interfaces not defined or delegated by the task. `Report`, `TrackedLv`, `Trend` and `write` are new public exports through the public `render::lvm` module, although W4-A defines no external renderer interface. These are useful implementation details, and no current caller outside the CLI crate needs them. Change the helpers and their exposed fields to `pub(crate)` where cross-module access is needed. This avoids expanding the Rust public surface without changing JSON or text behavior.

**Checked and found correct**

- The collector introduces one production spawn site. Its only call sites select `/usr/sbin/lvs` and `/usr/sbin/vgs`; arguments exactly match `07` §1, including kernel major/minor. No shell or `--readonly`; `LC_ALL=C`, `LVM_SUPPRESS_FD_WARNINGS=1`, and null stdin are set. Both streams are nonblocking, capped at 16 MiB, and drained in bounded batches. Error paths kill and wait for the child. Existing timeout/PID-reaping and output-cap tests passed. The override guard is `cfg!(any(debug_assertions, feature = "privileged"))`, so an ordinary release build cannot honor the two binary overrides. This last check was source inspection, not execution of a release binary. The pre-existing `rpm` spawn in `probe.rs` is outside the candidate diff.
- Parser probes cover truncated LV/VG JSON, missing sections, nonstring/null values, unknown nested fields, empty optional fields, negative/overflowing sizes, `u64::MAX`, locale-comma/nonfinite/out-of-range percentages, inactive/empty/overflowing kernel numbers, input/row caps and excessive JSON depth. All returned the expected result without panic. Device-number mapping ignores source spelling, preserves raw bracketed names for matching, orders paths by bytes, excludes unmatched devices, uses pool percentages, and deduplicates samples for bind mounts.
- Public filesystem signatures and field lists match W4-A, including the shared `VgInfo` re-export. No core/config/CLI flag/daemon API changes or new dependencies. Probe changes are confined to its LVM section and one-collection cache; absent and failed LVM remain advisory.
- CLI tests and additional process probes verify schema-wrapped JSON, path objects, LV size/type, pool usage, VG headroom and other LVs. Corrupt history warns without losing current reporting. Read-only history queries exclude samples older than 24 hours; three hourly points produced the expected 24 percentage-points/day slope. Missing tools and unmapped mounts exit 0 with `n/a`; a failed reporting command exits 1.
- No introduced unsafe code, production panic/unwrap/expect, filesystem mutation commands, scanned-file content reads, depth recursion or unbounded channels. Parser/process caps and the existing store query cap bound the new reporting inputs. Sorting and injected collector timestamps are present. This change adds no scan traversal, so scan-only Gate, symlink and O_NOATIME rules introduce no new call sites to audit.

**Fixtures and limits**

The synthetic fixtures have the expected `report`/`lv` or `vg` arrays, string values and empty optional fields. Their shape agrees with the [EL8 LVM JSON documentation](https://docs.redhat.com/es/documentation/red_hat_enterprise_linux/8/html/configuring_and_managing_logical_volumes/proc_diplaying-lvm-output-in-json-customizing-lvm-display). [Upstream LVM reporting code](https://gitlab.com/lvmteam/lvm2/-/raw/main/lib/report/report.c) uses the same name formatter for LV and pool names, supporting raw bracketed-name matching. Fixture attributes are simplified; this is not evidence of a captured lvm2 2.03 report.

No privileged tests, real LVM commands or VM access were attempted. EL8 captures, root/data runtime comparisons, real thin-pool behavior and the <2-second collection acceptance remain pending VM. No PostgreSQL functionality was reviewed. Cargo-deny was not reached because full CI stopped at Clippy.

A configuration probe also confirmed that default CLI selection follows `roots`, not `sampler.mounts`: `roots=[]` with a configured sampling mount yields `n/a`, while explicit `--mount` maps it. The plan labels roots as tracked filesystems and sampler mounts as statvfs inputs without defining a separate LVM selection list. This is an unresolved specification ambiguity, not a graded finding.

**Build and test evidence**

Used pinned Rust 1.98.0, a private copy of the host Cargo registry cache, `CARGO_NET_OFFLINE=true`, and missing-binary LVM overrides for general checks. Process tests/probes used temporary fake tools. Evidence is under this attempt's worker-output `library/` directory.

| Check | Observed result | Artifact |
|---|---|---|
| `tools/ci.sh` | Exit 1; source checks passed (134 owner tags), formatting passed; stopped at unrelated W1-B test compilation during Clippy | `ci.log` |
| `cargo test --offline --workspace` | Exit 101; same unrelated compilation errors | `workspace-tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 101; same unrelated compilation errors | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | `doc.log` |
| `cargo fmt --check` | Exit 0 | `fmt.log` |
| Clippy for FS/CLI libraries and CLI `--test w4a`, with `-D warnings` | Exit 0 | `lvm-clippy.log` |
| `cargo test --offline -p tidemark-fs --lib lvm::tests` | 6 passed | `lvm-fs-tests.log` |
| `cargo test --offline -p tidemark-cli --lib cmd::lvm::tests` | 2 passed | `lvm-cli-unit-tests.log` |
| `cargo test --offline -p tidemark-cli --test w4a` | 4 passed | `lvm-cli-process-tests.log` |
| `cargo test --offline -p tidemark-fs --features privileged --lib --no-run` | Exit 0; compiled the three privileged LVM tests, did not run them | `lvm-privileged-compile.log` |
| Additional parser/mapping probes | 31 passed | `parser-probes.rs`, `parser-probes.log` |
| Additional CLI/history probes | Outcomes described above | `cli-probes.py`, `cli-probes.log` |

The unrelated blocker is `crates/tidemark-fs/tests/w1b/main.rs:15` (missing `max_record_bytes`) and `:599`/`:726` (`&str` patterns against `WalkError::Limit(String)`). These are not findings against W4-A. Full workspace CI and test success are not claimed.

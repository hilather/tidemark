# Handoff — W1-A: Mounts, block devices, canonical paths, probe

## Needs from coordinator

1. Decide [SCR-001](scr-W1-A/SCR-001.md): valid nsfs object roots cannot inhabit the frozen `MountInfo.root: RawPath`. This currently makes mount discovery unknown in namespaces containing those records, including this sandbox. No fabricated paths or silently discarded mounts were introduced.
2. Update the out-of-scope retained stub assertion in `crates/tidemark-cli/tests/surface.rs:215`, as requested in [SCR-002](scr-W1-A/SCR-002.md). It expects `probe` to exit 1; the implemented report exits 0. This is the observed failure in workspace tests and CI.
3. Run VM validation on Rocky 8.8/8.10; supply `TIDEMARK_W1A_MD_DEVICE` pointing to a disposable existing RAID array for the md acceptance test. Genuine EL8 mountinfo captures remain needed.

- **Branch:** hp-c05d14c6a3dd5895c41b0f4b770e81a580aa69ce76afe4609db6aa8b497058fe
- **Agent/session:** Codex / attempt-c05d14c6a3dd5895c41b0f4b770e81a580aa69ce76afe4609db6aa8b497058fe
- **Date:** 2026-10-11
- **Status:** core implemented; evidence candidate with unresolved contract/test-maintenance gaps and pending VM acceptance. Overall task success is not claimed.

## What was built

Implemented pure mountinfo parsing with octal decoding, byte paths, device numbers, mount prefix queries, deterministic ordering and overmount tie handling. Implemented iterative, bounded sysfs graph traversal through dm/md and partitions to unique whole-disk leaves, plus exactly ordered disk-stat sampling that fails atomically on missing or malformed samples. Canonicalization resolves existing user paths while preserving non-UTF8 bytes. The read-only probe gathers host, capacity, mount, leaf/RAID, XFS and fanotify facts, explains unknowns, and derives deterministic recommendations without I/O. CLI output includes grouped text with markers and `tidemark.probe.v1` JSON, with PostgreSQL composed only in the CLI and LVM explicitly n/a while W4-A remains a stub.

## Shared-file edits

| File | Change | Why |
|------|--------|-----|
| crates/tidemark-cli/src/render/mod.rs | Added documented `pub mod probe;` declaration | Explicitly authorized renderer declaration; required by missing-docs lint |

No dependencies, manifests, lockfiles, crate roots, core contracts, plan files or other task implementations changed. All other edits are owned paths.

## Deviations from the spec

| Spec reference | What was done | Why | SCR? |
|----------------|---------------|-----|------|
| W1-A captured EL8 mountinfo samples | Four synthetic recorded-format sample files, labeled in data/README.md | No VM or genuine EL8 captures available; no invented provenance | No |
| MountInfo.root absolute RawPath | Explicit parse error on nsfs object roots, with explained probe unknowns | Frozen field cannot represent valid kernel records | SCR-001 pending |
| Finish: workspace tests / CI green | Preserved the stale out-of-scope assertion and recorded failed checks | Changing it is prohibited by this attempt's write scope | SCR-002 pending |
| md VM acceptance | Read-only test against an existing VM array, using lsblk as independent oracle | No md fixture helper exists; avoid creating arrays on this host | No; VM provisioning required |
| Host CPU count | Process-available parallelism, rather than physical sockets/cores | Safe std API, subject to affinity/cgroup constraints | No |
| String mount source | Escaped display for non-UTF8 absolute sources; unknown/error for non-UTF8 non-path sources | Frozen String field; preserve meaningful path bytes in display | No |
| LVM section | n/a with explicit W4-A explanation | Assigned task explicitly requires n/a while LVM is stubbed | No |
| Offline builds | Pinned Rust 1.98.0 on PATH, copied registry cache at /tmp/tidemark-w1a-cargo, offline environment | crates.io unavailable; host cache not modified | No |

All maps/queues in new discovery logic have documented limits: mountinfo 16 MiB, sysfs attribute 64 KiB, device graph/requested samples 4096, host metadata 1 MiB, probe roots 1024. RPM discovery has a two-second subprocess deadline. No recursion follows device graph depth. Sysfs topology symlinks and explicitly requested root canonicalization are intentional; no scanner traversal was added.

## Acceptance criteria evidence

Evidence directory: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-c05d14c6a3dd5895c41b0f4b770e81a580aa69ce76afe4609db6aa8b497058fe/library/`.

| Criterion | Evidence | Status |
|-----------|----------|--------|
| `tidemark probe` on VM prints every section; unknowns explained, never panics | `json_snapshot_and_text_unknowns`, `probe_json_has_all_sections_and_unknown_postgres`; local probe-host.txt / probe-host.json exit 0 | Local evidence passes; pending VM; nsfs gap disclosed |
| `resolve_leaves` on md VM returns member disks | `dm_md_partitions_resolve_to_unique_sorted_disks` passes; `resolve_vm_md_array_matches_lsblk_members` compiled | pending VM |
| JSON validates against insta snapshot | `json_snapshot_and_text_unknowns`; committed probe_v1 snapshot includes path_b64, unknown reasons and PostgreSQL section | Local pass |
| Correct loop-XFS geometry | `probe_loop_xfs_geometry_matches_xfs_info`, comparing block and inode sizes with xfs_info | Compiled; pending VM and W1-C integration |
| Loop-LVM resolves loop PVs | `resolve_loop_lvm_returns_pv_disk`, lvs reporting oracle | Compiled; pending VM |
| Real same-device bind mount visible in both mount queries | `real_bind_mount_is_a_same_device_submount` | Compiled; pending VM |

## Validation performed

Final source checks were rerun after source changes. Each command has a `.log` and `.exit` artifact in the evidence directory.

| Command | Exit / outcome | Artifact |
|---------|----------------|----------|
| cargo fmt --check | 0 | fmt.log |
| cargo clippy --offline --workspace --all-targets -- -D warnings | 0 | clippy.log |
| cargo test --offline --workspace | 101; `binaries_and_bash_completions` expected 1, observed 0 | test.log |
| cargo build --offline --workspace --features privileged --tests | 0; privileged tests compiled, not executed | privileged-build.log |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | 0 | doc.log |
| tools/ci.sh with CARGO_NET_OFFLINE=true | 1; stops at the same retained test assertion | ci.log |
| cargo test --offline -p tidemark-fs | 0; 17 unit tests and 9 existing integration tests pass | w1a-fs.log |
| cargo test --offline -p tidemark-cli --lib --test w1a | 0; 6 library tests and 1 W1-A integration test pass | w1a-cli.log |
| cargo test --offline --workspace -- --skip binaries_and_bash_completions | 0; 106 passed, one existing scale test ignored, one stale assertion filtered | workspace-supplemental.log |

The supplemental run is not a passing result for the required unfiltered workspace command. CI did not reach cargo-deny, so no dependency advisory/license audit is claimed. No privileged test or destructive host command was executed.

## Tests added

Unprivileged (all pass):

- `mountinfo_samples_and_bind_mounts`
- `malformed_records_are_errors`
- `el8_shaped_lvm_md_and_bind_records`
- `nsfs_nonabsolute_root_requires_contract_decision`
- `dm_md_partitions_resolve_to_unique_sorted_disks`
- `disk_stat_variants_and_invalid_input`
- `samples_preserve_order_duplicates_and_fail_atomically`
- `missing_attributes_are_unknown_and_cycles_are_bounded`
- `resolves_links_and_preserves_bytes`
- `advisory_rules_are_independent_and_unknowns_do_not_warn`
- `host_metadata_parsers`
- `missing_root_is_reported_without_panicking`
- `json_snapshot_and_text_unknowns`
- `probe_json_has_all_sections_and_unknown_postgres`

Privileged, gated only by `--features privileged`, compiled and **pending VM**:

- `probe_loop_xfs_geometry_matches_xfs_info`
- `resolve_loop_lvm_returns_pv_disk`
- `real_bind_mount_is_a_same_device_submount`
- `resolve_vm_md_array_matches_lsblk_members`

## Known gaps and follow-ups

- Mount discovery is unavailable when any record has an unrepresentable root; SCR-001 must be resolved before claiming broad namespace compatibility.
- Full workspace tests and CI are red until the coordinator maintains the retained CLI test. No out-of-scope test edits or exit-status workaround were used.
- XFS geometry is still a W1-C stub on this baseline; geometry errors and bulkstat placeholder values remain unknown instead of false capabilities. Fanotify and PostgreSQL stub errors retain NotYetImplemented explanations.
- Real Rocky captures, VM geometry/LVM/bind/md results, Rocky release warnings and hardware scheduler findings remain unverified. Samples are synthetic.
- LVM probe wiring beyond the requested temporary n/a behavior belongs to W4-A integration.
- CPU count is available parallelism. Cgroup v1 on tmpfs is reported as `1 (or hybrid)`; precise hybrid layout enumeration is not implemented.
- Probe metadata parsers support ordinary EL8 os-release quoting; full shell-style os-release escaping is not implemented.

## Notes for reviewer

Public cross-task signatures and MountInfo/ProbeReport field lists remain unchanged; the formerly unspecified fact types now have serializable shapes. Root and mount-target JSON use path/path_b64 and same-device mounts preserve bytes; absent mount-table results are represented as unknown, including the other-mounts list. Disk sampling preserves duplicate requests and positional order. Pure advisory rules deduplicate by code, sort deterministically and do not infer negative capabilities from unknown facts. Canonicalization is performed once before probing and is distinct from the future scanner's no-follow traversal.

## Definition of Done checkpoint

- [x] Owned paths only, plus authorized renderer declaration listed above.
- [x] Format, Clippy, strict rustdoc and focused tests pass locally.
- [x] Privileged tests written and compiled, listed as pending VM.
- [x] Public items documented; owner headers present; no unsafe added.
- [ ] Full CI green: blocked by retained test outside write scope.
- [ ] All VM acceptance achieved: pending VM.
- [ ] nsfs root contract resolved: pending coordinator decision.

This handoff is evidence for review, not a verified success or integration decision.

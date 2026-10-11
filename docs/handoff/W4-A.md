# Handoff — W4-A: LVM collector, CLI and probe

## Needs from coordinator

- Provide an EL8 disposable VM for real report captures and acceptance measurements. Synthetic fixtures explicitly identify real captures as pending VM.
- No shared-file edits or new dependencies are needed.

- **Branch:** hp-fc8ecceebe61b380fecb4cd093a197b5506712cce689cae4eef26355845dafe5
- **Attempt:** attempt-fc8ecceebe61b380fecb4cd093a197b5506712cce689cae4eef26355845dafe5
- **Date:** 2026-10-11
- **Status:** implemented; evidence candidate awaiting independent verification and VM acceptance.

## What was built

Implemented defensive pure parsers, deterministic major/minor mount mapping, thin-pool-based current state and deduplicated per-LV samples. The collector launches only the exact two absolute reporting commands, without a shell or `--readonly`, with the required environment; debug/privileged builds permit fake executable overrides. Nonblocking stdout/stderr draining enforces a 16 MiB cap per stream, 16 MiB parser-input cap and 100,000-row cap. A deadline kills and reaps the reporting child on timeout or output/I/O failure.

The direct CLI renders tracked LV size/type, pool percentages, relevant VG headroom and other LVs, including schema-wrapped JSON and byte-preserving mount objects. Optional read-only timeseries history supplies least-squares percentage-point/day trends over the last 24 hours, requiring at least three valid samples. Missing or corrupt history leaves current reporting usable. Probe collects at most once across roots, reports LV/VG/pool/headroom, and keeps errors advisory.

## Shared-file edits

None. The only probe changes are the LVM cache declaration and the existing LVM facts section. No dependencies, manifests, lockfiles, crate roots or shared module declarations changed.

## Deviations from the spec

| Reference | Choice | Reason | SCR |
|---|---|---|---|
| W4-A captured fixtures | Handwritten synthetic lvmreport JSON | Explicit task instruction; no VM exists | None |
| W4-A trend wording | Least-squares slope in percentage points/day, three samples minimum | Consistent with live rate policy; no prior exact trend format specified | None |
| Probe sampling timestamp | Inject 0 for its metadata-only collection | Probe exposes no LVM sampling timestamp; avoids clock I/O in probe logic | None |
| Bounded reporting | 16 MiB input/stream cap; 100,000 rows; timeout applies independently to each reporting child | Documented local constants; frozen config has no report cap | None |
| Optional historical database | Open only when present, read-only; log and omit unavailable history | Current LVM reporting remains usable without history | None |
| Build environment | Pinned toolchain binaries on PATH, copied host registry cache under /tmp, offline Cargo | Cargo absent from worker PATH and projected Rust home is read-only | None |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| VM root and /data CLI matches lvs/vgs | Synthetic end-to-end CLI/probe test plus privileged `lvm_maps_mounts_to_lvs`, with measured VG-free and LV-size oracles | pending VM |
| collect takes < 2 s on VM | Privileged `lvm_maps_mounts_to_lvs` measures and asserts elapsed time < 2 s | pending VM |
| Parser coverage | `parser_fixture_types_and_empty_fields`; `parser_rejects_bad_shape_numbers_and_nonfinite_percentages` | passed locally |
| Device-number mapping across source spellings | `maps_device_numbers_and_uses_pool_usage` | passed locally |
| Hard timeout and no zombie | `timeout_kills_and_reaps_child` measures deadline and verifies reporting PID disappeared from /proc | passed locally |
| Missing tool is n/a | `command_environment_arguments_success_and_failure`; CLI `missing_tools_render_na_text_and_json` | passed locally |
| History optional and 24-hour trend | `renders_text_json_and_absent_history`; `reads_history_and_computes_24h_trend` | passed locally |
| Existing probe remains usable | Existing W1-A tests plus `probe_tolerates_absent_or_failed_lvm` | passed locally |

## Tests

Unprivileged source-module tests:
- FS: `parser_fixture_types_and_empty_fields`, `parser_rejects_bad_shape_numbers_and_nonfinite_percentages`, `maps_device_numbers_and_uses_pool_usage`, `command_environment_arguments_success_and_failure`, `timeout_kills_and_reaps_child`, `output_cap_and_disabled_collection`.
- CLI: `renders_text_json_and_absent_history`, `reads_history_and_computes_24h_trend`.

Unprivileged process-isolated CLI tests:
- `missing_tools_render_na_text_and_json`, `fake_executables_are_collected_without_host_lvm`, `probe_tolerates_absent_or_failed_lvm`, `synthetic_report_maps_measured_fixture_device_in_cli_and_probe`.

Privileged tests (feature-gated, no ignore):
- `lvm_maps_mounts_to_lvs` — pending VM.
- `lvm_thin_pool_usage_tracks_writes` — pending VM.
- `lvm_absent_is_na` — pending VM.

No tests ignored pending another task. Privileged tests were not executed on this host. General checks use missing-binary overrides; task process tests override them with temporary scripts. No host LVM reporting or mutation command was executed.

## Validation

All commands completed with exit 0, with Cargo offline against the copied cache. General checks set `TIDEMARK_LVS_BIN=/missing/tidemark-lvs` and `TIDEMARK_VGS_BIN=/missing/tidemark-vgs`; process-isolated task tests supply their own scripts.

| Check | Observed result | Output artifact |
|---|---|---|
| `cargo fmt --check` | exit 0 | library/fmt.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 on final source | library/clippy.log |
| `cargo test --offline --workspace` | exit 0; final-source workspace tests also pass inside CI | library/test.log; library/ci.log |
| `cargo build --offline --workspace --features privileged --tests` | exit 0 on final source; compiled only | library/privileged-build.log |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 on final source | library/doc.log |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | exit 0; 129 owner tags; forbidden-symbol scan passed; all available checks passed | library/ci.log |

Cargo-deny is not installed; the CI wrapper explicitly skipped that audit. No VM-dependent acceptance item is claimed passed.

## Measurements

No real LVM performance or VM measurement is claimed. The fake-child timeout test uses a 100 ms deadline with an outer < 2 s assertion. Real collection timing, kernel/platform details and reference-command comparison must be recorded at the VM gate.

## Known gaps and follow-ups

- Real EL8 report captures are not provided: pending VM.
- Root and /data acceptance, thin-pool runtime test and < 2 s collection measurement are pending VM.
- Daemon sampling interval/backoff and daemon-mode CLI routing belong to later integration tasks.

## Notes for the reviewer

Public signatures and field lists are unchanged. Raw bracketed LV names remain available for matching; terminal rendering strips display brackets and escapes controls. Thin percentages are read from the pool, not the thin LV's own data percentage. Bind mounts are deduplicated when producing samples. No unsafe code, async runtime, new dependencies or scanned-file content reads were introduced. Child stdout/stderr are nonblocking and drained in bounded batches so continuous output cannot defeat the deadline.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits.
- [x] Required local checks completed (cargo-deny unavailable; see validation results).
- [x] Privileged tests written and listed as pending VM.
- [x] Public items documented; owned-source headers present.
- [ ] All VM acceptance criteria evidenced at runtime.
- [x] No unexplained TODOs or production unwrap/expect calls.

# Handoff — W1-A-fix: mount roots, resolution and display

- **Attempt/branch:** attempt-1aba0f0f891ea9f02b99e02f4043529f62135024ad362ff32abf946858281bef / hp-1aba0f0f891ea9f02b99e02f4043529f62135024ad362ff32abf946858281bef
- **Date:** 2026-10-11
- **Status:** implementation and local validation evidence submitted for review; verified task success and integration are not claimed. VM acceptance remains pending.

## Changes and evidence

1. Implemented the task-authorized SCR-001 enum `MountRoot::{Path, Object}`. Object bytes survive decoding exactly, including non-UTF8 bytes. nsfs targets remain mount boundaries and participate in all unchanged query signatures. `nsfs_object_root_keeps_table_and_boundaries` verifies preservation, selection, submounts and unrelated-device queries. `malformed_records_preserve_neighbors` verifies malformed numbers, fields and escapes cannot discard good records. Each rejected line emits a structured `tracing::warn!` with its 1-based line number and parse reason; it does not log raw potentially hostile bytes. The existing 16 MiB table cap still rejects oversized input as a whole. The retained SCR document says Pending; the explicit assigned task accepts the proposal and authorizes this change.
2. Configured roots reach canonicalization as original PathBuf values. CLI paths are composed into a cloned config using a private helper, preserving original bytes and path components until the collector resolves them. The frozen public `probe` signature is unchanged. `configured_paths_resolve_before_normalizing`, `cli_paths_resolve_before_normalizing`, and `executable_cli_and_config_paths_use_realpath` cover both input routes, local symlink/parent traversal, traversal into `/proc`, and missing intermediate components. The executable regression runs all six path/input combinations.
3. Mount sources and option strings escape controls, literal backslashes and invalid UTF-8 using the same byte-display algorithm as RawPath, preserving printable Unicode. Mount target display still uses RawPath; filesystem type facts also pass through the escape helper. Options are escaped once during parsing; recognized ordinary option names retain their semantics. `parsed_mount_fields_cannot_forge_text_lines` connects parser output to text rendering for newline, ESC and backslash in source, mount target and both option lists, preserving `é`. No new dependency was added.
4. A canonicalization failure prevents mount lookup, block-device/XFS/fanotify discovery and statvfs for that root. All root facts, including LVM, are unknown with the original failure reason, and mount_point/leaves/other_mounts are absent. The renderer uses that reason for unknown other mounts. `missing_root_is_reported_without_panicking` and the missing-intermediate regressions assert this behavior. RawPath is constructed on failures only as a report label; it is never used for discovery. Deduplication distinguishes failed labels from successful canonical roots, so a failed normalized label cannot suppress a valid root.
5. Synthetic inputs remain active. The fixture README explicitly calls real EL8 captures an unmet acceptance input pending VM access. `captured_el8_inputs_pending_vm` prints that skip explanation and returns when the capture directory is absent; when supplied it requires 3–4 files and parses each. Captures and provenance are **not supplied**, and VM acceptance remains unmet.

## Validation

Used pinned Rust 1.98.0 on PATH, CARGO_NET_OFFLINE=true, and CARGO_HOME=/tmp/tidemark-w1afix-cargo copied from the host registry/git caches. No host cache changes, privileged test execution, mount changes or destructive commands occurred.

| Check | Observed result |
|---|---|
| cargo fmt --check | 0; final CI also checks format |
| cargo clippy --offline --workspace --all-targets -- -D warnings | 0 final rerun; initial test initializer lint fixed |
| cargo test --offline --workspace | 0; final CI repeats with the final executable regressions |
| cargo build --offline --workspace --features privileged --tests | 0; compiled only |
| tools/ci.sh | 0; all available checks passed |
| tidemark probe / tidemark probe --json | Both exit 0; default missing /data is unknown and / has known mount facts |
| Supplemental --root / text and JSON probes | Both exit 0 |

CI explicitly skips cargo-deny because it is not installed; no advisory/license audit is claimed. Existing W1-A privileged loop-XFS, loop-LVM, real bind and md tests compile and remain **pending VM**. Two unrelated scale tests remain ignored. The existing probe_v1 insta snapshot passes unchanged: its fixed mock facts do not change with these fixes. Reviewed the snapshot by eye; no changed or pending snapshot files exist.

Artifacts: /home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-1aba0f0f891ea9f02b99e02f4043529f62135024ad362ff32abf946858281bef/library. Final CI transcript is ci.log; final explicit lint transcript is clippy-final.log; command outcomes and complete host text/JSON outputs are retained there.

## Local probe excerpts

Default `tidemark probe` (shortened):

```text
Root /data
  ? canonical_path: unknown (No such file or directory (os error 2))
  ? fs_type: unknown (No such file or directory (os error 2))
  ? mount_point: unknown (No such file or directory (os error 2))
  ? other mounts: unknown (No such file or directory (os error 2))

Root /
  ✓ atime: relatime
  ✓ fs_type: btrfs
  ✓ mount_point: /
  ✓ source: /dev/mapper/root
```

Default `tidemark probe --json`, root `/` facts (shortened):

```json
{
  "mount_point": {
    "value": "/",
    "why": null
  },
  "fs_type": {
    "value": "btrfs",
    "why": null
  },
  "source": {
    "value": "/dev/mapper/root",
    "why": null
  },
  "atime": {
    "value": "relatime",
    "why": null
  }
}
```

## Scope and remaining uncertainty

Only owned source/tests/data and this handoff changed. No shared-file edits, manifests, lockfiles, crate roots, dependencies, core contracts, plan files or other task implementations changed. blockdev.rs and canon.rs needed no changes. MountRoot is the explicitly authorized contract amendment.

Real EL8 captures, Rocky/XFS/LVM/md/bind VM runtime behavior and privileged ioctl behavior remain unverified. Fanotify/PostgreSQL/LVM implementation integration remains outside this fix's scope. These local checks and outputs are review evidence, not a release or integration decision.

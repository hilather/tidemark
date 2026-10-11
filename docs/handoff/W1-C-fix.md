# Handoff — W1-C-fix: v5-only bulkstat and complete ABI assertions

- **Agent/session:** Codex, attempt-f20731b398697271609be6a9c8792147aa8441f9d353ba365d9d8263631b7bf4
- **Date:** 2026-10-11
- **Status:** implementation and local validation evidence candidate; independent verification and runtime acceptance pending

## What was built

The blocker is addressed by removing the v1 metadata backend, conversion, buffers and forced-v1 metadata test. BulkInode link counts come exclusively from v5's 32-bit bs_nlink. The independent v1/v5 capability probe remains; BulkstatSupport documents that bulk mode requires v5. All public signatures and public field lists remain unchanged.

ENOTTY, EINVAL, EOPNOTSUPP and ENOSYS from the v5 request produce Unsupported with the message: "v5 bulkstat is unavailable; v1 is not used because its link counts are 16-bit". ENOTTY and EINVAL cover unrecognized ioctl dispatch; EOPNOTSUPP and ENOSYS preserve the previous unsupported-interface classifications without enabling legacy fallback. EINVAL is conservatively treated as unavailable for this validated, bounded, zero-reserved request; it cannot distinguish unknown dispatch from other kernel rejection. Other errors retain their OS errno, including EPERM. The error is yielded once and the iterator terminates; callers can choose statx walk mode.

The should-fix is addressed by asserting size, alignment and every declared field offset of all eight FFI structs. Geometry tests now cover all fields, and the v5 layout includes bs_extents64 at 136 and bs_pad at 144. A flexible-array request struct covers hdr at 0 and bulkstat at 64. The binding header records EL8 and EL9 measured layout provenance. Legacy structs and ioctl assertions remain for probing and ABI regression protection.

## Shared-file edits

None. No dependencies or contract/spec files changed.

## Deviations from the spec

The accepted task decision supersedes the older v1 metadata fallback prose in scanner §5. No additional contract change is proposed. Builds use Rust 1.98.0 and an offline temporary copy of the host Cargo registry, following W0-A; the initial default-cache invocation could not resolve base64.

## Acceptance criteria evidence

- `unavailable_v5_never_returns_legacy_metadata`: mocked v1 request succeeds, while each unsupported v5 errno yields Unsupported, the message names the 16-bit reason, zero v1 calls occur during iteration and no records follow.
- `v5_preserves_wide_links_and_project_id`: retains the 70,000-link test and metadata/overflow checks.
- `v5_permission_errors_keep_errno`: EPERM remains PermissionDenied with its original errno and terminates once.
- `rocky8_x86_64_layouts` and `upstream_v5_layout`: complete field offset, size and alignment assertions, including padding/reserved fields and flexible array.
- Existing field-width and ioctl-number tests remain; v5 inumbers and flag/version constants are also asserted.
- `cargo fmt --check`, offline workspace Clippy with `-D warnings`, offline workspace tests, offline privileged-feature test build and `tools/ci.sh`: exit 0. Transcripts and exit statuses are in the attempt output's library directory.

## Tests and remaining uncertainty

All real XFS ioctl integration tests remain feature-gated and **pending VM**. The probe integration test now explicitly requires v5 support. No privileged tests, mounts, LVM operations or host configuration changes were executed. Kernel availability and block-unit equality on Rocky remain runtime checks. CI skips cargo-deny if unavailable; this does not establish advisory/license audit completion.

## Notes for the reviewer

Test-only thread-local mocks do not change production dispatch. Geometry is supplied directly in mocked iterator tests so they execute no real ioctls. All touched unsafe blocks remain in tidemark-sys and carry SAFETY comments. Memory remains bounded by the existing batch cap. No shared modules, manifests, lockfiles or other task files changed.

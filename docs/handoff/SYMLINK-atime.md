# Handoff — SYMLINK-atime: Mount-contained symbolic-link target checks

- **Branch:** precreated attempt branch `hp-26e9e2e078d189de11c0adc3ea6008412264d2c13b09ff478fc33354a6cda0b1`
- **Agent/session:** Codex, attempt-26e9e2e078d189de11c0adc3ea6008412264d2c13b09ff478fc33354a6cda0b1
- **Date:** 2026-10-13
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed

Replaced the following target `statat` with iterative, descriptor-relative component resolution. Each component is checked with non-following, no-automount `statx`, then pinned using `O_PATH|O_NOFOLLOW` and revalidated before reading link text or opening a directory. Text is read from the pinned symlink with empty-path `readlinkat`; enumerated links are pinned and revalidated too when target checking is enabled; lexical-only mode preserves its existing syscall budget. A retained mount-root handle anchors target resolution. Relative targets, absolute targets within that mount, intermediate symlinks, dot-dot after symlink expansion, directory requirements and final classification use actual component resolution rather than premature lexical normalization.

Mountinfo identifies the selected path's mount and all nested mount boundaries, including same-device bind mounts. Known boundaries stop before a target syscall; unexpected device/mount-ID boundaries stop after the non-following metadata check. Absolute paths outside the selected mount and dot-dot leaving it return `OtherFs` immediately, without checking the remainder. Targets outside the query subtree but still inside the selected mount remain checkable. Cancellation is propagated; all metadata operations are charged through the Gate. Directory opens retain `O_NOATIME` with charged EPERM fallback. Chains are limited to 40 link expansions, 4096 retained directory handles, 1 MiB aggregate retained directory-path bytes and 1 MiB pending text; cap errors other than the Linux-style link limit classify as unreadable.

The command help documents possible link-atime updates. The check-targets help explains mount containment and unchecked other-filesystem targets. The flag's definition actually lives in `cmd/symlinks.rs`, so its one help sentence was changed there; `cli.rs` contains the command sentence. No behavior or rendering outside the existing state was changed.

## Shared-file edits and contracts

No dependencies, manifests, module declarations, core types, configuration keys or schemas changed. Only the five authorized paths changed. Documentation proposals below were not applied to the concurrently owned operator guide or the plan/security documents.

The existing result schema already represents boundary departures with `OtherFs`, human `OtherFs`, JSON `other_fs`, and a separate `by_class` total. The command's existing escaping filter includes that class, and its dangling filter excludes it. No new JSON fields or class variants were added.

## What an operator loses

`OtherFs` now means a target left the selected mount and was not checked beyond that boundary. It does not establish whether the external target exists, dangles or loops. The frozen result cannot distinguish a device change from a same-device bind mount or an absolute/relative exit, carry a reason string, or report the final external target. Counts are per original enumerated link, not per intermediate link read. The 40-expansion limit retains Linux ELOOP-style reporting for very long chains as well as actual cycles.

## Acceptance evidence

- `selected_mount_boundary_stops_before_external_metadata_or_link_text`: temporary root substituted for a mount through a private test-only seam; relative exit, absolute exit, second-link exit, internal dangling chain, internal loop, absolute internal chain and a non-directory dot-dot case. Recorded target metadata/readlink/open paths all remain inside the substituted boundary and never reach the external sentinel.
- Existing unprivileged `w3b` tests retain byte paths, lexical mode, cancellation/caps, gated retry charges, non-descent through symlinks, filtering and dot-dot-after-symlink semantics. The old one-stat-per-link charge assertion now expects additional charged component work.
- `cross_mount_chain_preserves_second_link_atime_pending_vm`: two loop XFS mounts using strictatime, link on the first through a link on the second, old second-link atime including nanoseconds. It asserts `OtherFs`, zero dangling/loop counts, and unchanged second-link atime. Compiled only here; **pending VM**.

## Validation

Validation uses Rust 1.98.0, `--offline`, and a temporary Cargo home linked to the existing cache. Every Cargo command is wrapped in `timeout 1500`. The final source tree produced these local results (logs in the attempt output `library/final/`):

| Command | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | Exit 0 |
| `cargo test --offline --workspace --no-fail-fast` | Exit 0; all executed tests passed; scale tests retain their declared ignores |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; no feature-enabled tests executed |

The initial workspace run found an exact lexical-only syscall-budget regression in `crates/tidemark-fs/tests/rules.rs:213`. It was fixed within the owned source by preserving lexical-only reads, and the final workspace run passed that test and all seven fs `w3b` tests. The daemon/remount and socket readiness failures anticipated by the task brief did not reproduce in the final run. These are local command observations, not independent task verification or release evidence.

Guest-only command (run as root in a disposable VM, never on the host):

```sh
timeout 1500 cargo test --offline -p tidemark-fs --features privileged --test w3b cross_mount_chain_preserves_second_link_atime_pending_vm -- --exact --nocapture
```

## Proposed text

For `docs/operator-guide.md`:

> `tidemark symlinks` reads symbolic-link text and can cause the kernel to update the access time of links it reads, subject to the mount's atime policy: never with `noatime`, and with default `relatime` normally at most about once a day per link. Nothing else in Tidemark reads filesystem symbolic-link text; scans do not read link text. With `--check-targets`, targets are resolved component by component only inside the selected path's mount; links leaving that mount, including through bind mounts or an intermediate link, are reported as `OtherFs` (`other_fs` in JSON), counted separately, and not checked for dangling or looping state beyond the boundary.

For `docs/plan/03-link-semantics.md` §5:

> Scanners never read symbolic-link text or follow symbolic links. The live `symlinks` command reads link text and may thereby cause kernel link-atime updates according to the mount's atime policy; parent-directory `O_NOATIME` does not suppress those updates. Optional target checking resolves components with non-following metadata checks and explicit link expansion inside the selected mount, stops before following any boundary departure, and classifies the original link as `OtherFs`, with external dangling or looping state not checked.

For `docs/plan/08-interfaces.md`:

> `symlinks` reads symbolic-link text and may update link access times according to the mount's atime policy (never with `noatime`; default `relatime` normally at most about once a day per link). `--check-targets` diagnoses targets only inside the selected path's mount; a chain leaving that mount is reported and counted as `OtherFs` (`other_fs` in JSON), meaning its state beyond the boundary was not checked, and is never classified as dangling or looping based on that external state.

For `docs/plan/12-decisions-and-risks.md`:

> Owner decision of 2026-10-13: `tidemark symlinks` continues reading symbolic-link text on every mount, with a documented exception for kernel link-atime updates governed by the mount's atime policy; Tidemark does not remount filesystems or restore timestamps. Target checking is confined to the selected path's mount, uses explicit component resolution rather than kernel-side link following, and reports boundary departures with the existing `OtherFs` class and separate counts; target state beyond the boundary is not checked. Scans never read link text and are unaffected by this exception.

For `docs/security.md`:

> The live `tidemark symlinks` command is the exception to the no-atime-write goal: reading symbolic-link text can cause the kernel to update that link's access time under the mount's atime policy, never with `noatime` and with default `relatime` normally at most about once a day per link. Parent-directory `O_NOATIME` cannot prevent link-atime updates; Tidemark never remounts or restores times to hide them. `--check-targets` expands chains under Tidemark's control with non-following component metadata checks inside the selected mount; on a device change, nested mount (including a bind mount) or exit from that mount it stops, reports `OtherFs` (`other_fs` in JSON), and leaves external dangling or looping state unchecked. Scans do not read link text, and no other Tidemark command reads filesystem symbolic-link text; procfs descriptor/namespace link text is kernel metadata rather than filesystem symlink analysis.

## Needs from coordinator

Apply the proposed documentation consistently, including the original W3-B description that still specifies following `stat`. Execute the pending guest regression. No CLI/man-page test pin for either changed help sentence was found by searching `crates/tidemark-cli/tests/**` and `xtask/tests/**`.

Existing EL8 statx mount-ID limitations still apply: mountinfo rejects known bind mounts, but a same-device bind introduced after discovery may be indistinguishable where the kernel provides no mount ID. Mount management is trusted; this change does not invent a new frozen syscall API to remedy that platform limitation.

Documentation reconciliation: the owner's requested “nothing else reads link text” wording above should be reviewed against `crates/tidemark-fs/src/canon.rs:7`, which delegates to `std::fs::canonicalize` and resolves administrator-supplied query-path symlinks before scanning/analysis, and against the kernel procfs link readers already described in the audit. The analyzer's mount containment starts after query-path canonicalization. I did not alter that shared, out-of-scope query-path behavior; the proposed exclusivity sentence describes filesystem-entry analysis and should not be interpreted as proof that no other path-resolution operation can traverse an administrator-selected symlink.


## Definition of Done evidence

- [x] Only the five authorized paths changed; no new dependencies or shared module edits.
- [x] Final format, both Clippy variants, workspace tests and privileged compilation exited 0.
- [x] Privileged atime regression written and compiled; execution remains pending VM.
- [x] Public production items documented; no unsafe or production unwrap/expect added.
- [x] Handoff records implementation, schema limitations, exact proposed text and follow-ups.
- [ ] Independent verification and guest atime evidence are coordinator responsibilities.

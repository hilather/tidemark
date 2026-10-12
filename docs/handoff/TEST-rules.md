# Handoff — TEST-rules2: unprivileged golden-rule regressions

## Needs from coordinator

Run `namespace_mounts_are_skipped` in the namespace-capable CI. This sandbox refuses the uid-map write before entering the child, so the real tmpfs/bind-mount assertions have not run here. Review the counting-gate limitation below: a new uncharged syscall cannot be detected by charge counts alone.

- Attempt: attempt-e357ad229ebe0ff4b9696808567ae2d07d021d7fed6a0985e8c42fec7034580f
- Base: a16769ac3549138091feb130f4ed3c6d502c4b0d
- Date: 2026-10-12
- Status: implementation evidence candidate; coordinator verification pending.

## What was built

Applied the supplied draft with `git apply`, reviewed it, and retained its behavior-preserving walker seams. Added six FS tests and two CLI tests. Boundary inputs cover equal identity, another device, same-device/different-mount identity and unavailable root mount IDs; fixture records cover Mount and Excluded reasons. The namespace child mounts tmpfs and binds an outside source under the fixture root, scans through the binary, and checks both Mount skips and zero contributions to file, allocated-byte and apparent-byte totals. Both names, not just skip counts, are asserted.

The NOATIME tests inspect flags through the actual attempt helper, owned-directory atime preservation, and real foreign-directory opens. CapEff bit 3 and the selected directory owner determine the fallback expectation. O_PATH ancestors ignore NOATIME; exactly one final O_RDONLY foreign directory is opened by this test. The test is active, with no early success or uid-zero assumption.

The metadata-only binary test uses mode-000 and readable regular files with old atimes, a hard-link alias and a symlink. It checks file counts, inode-based disk/apparent sizes and zero scan errors against fixture metadata, then checks unchanged regular-file atimes after scan, symlinks with target checks, and links with show-paths. Real-root runners that can bypass mode-000 drop uid/gid for those commands, using a temporary executable copy. No read privilege is inferred from uid alone. A fixture read before resetting timestamps establishes that contents reads actually update atime on this filesystem; failure of that precondition is a test failure, not a skip.

Gate expectations come from the two-directory/two-file fixture: one charge per component open (including /), identity stat, mountinfo read, getdents batch and EOF, and file stat. Short symlink targets add exactly one entry stat and one readlink to otherwise identical directory operations. Counts are independent of walker counters.

## Shared/source seam edits

No manifests, lockfiles, dependencies, module declarations outside walk, or contracts changed. Every production-source hunk is listed here (final line numbers):

| File | Lines | Behavior-preserving change |
|---|---|---|
| walk/getdents.rs | 55–68 | Document/export existing attempt helper; its private bool callback becomes OFlags::NOATIME/empty, exposing the existing first/retry decision. Charges and EPERM handling are unchanged. |
| walk/getdents.rs | 104–105 | OR the supplied flag into the same existing open flags instead of converting bool to flags locally. |
| walk/getdents.rs | 298 | Existing unit-test callback checks the equivalent flag bit. |
| walk/mod.rs | 95–96 | Re-export the attempt helper for integration tests. |
| walk/mod.rs | 102–115 | Extract existing device/known-mount predicate into documented hidden boundary_reason. |
| walk/worker.rs | 382–389 | Use that predicate for directory identity; preserve the root's exemption from mount-ID comparison. |
| walk/worker.rs | 609 | Use the same predicate for child attributes. |

Moved `7055/W5-A-fix2.md` and `7488/TEST-e2e-deploy.md` with git mv to docs/handoff, deleting only their sentences claiming the numbered location was required. Removed the empty numbered directories. Own note is docs/handoff/TEST-rules.md.

## Mutation evidence

Temporary mutations were compiled and executed individually, then restored byte-for-byte. No mutation is committed.

| Rule | Mutation | Observed evidence |
|---|---|---|
| Filesystem boundaries | Disable the extracted production predicate | boundary_decisions_and_skip_reasons fails: another-device input returns None instead of OtherFs; exit 101. |
| O_NOATIME | First attempt uses empty flags | noatime_attempt_flags fails: first flag is empty; exit 101. |
| Metadata only | Read regular-entry contents in the walker | regular_contents_are_never_read fails: readable file atime changes; exit 101. |
| Gate | Leave the existing per-entry stat syscall uncharged | exact_walk_syscall_budget_from_shape fails: two charges missing; exit 101. |

A counting gate detects an existing system call that stops being charged, not a new uncharged one. An additional mutation inserted a duplicate uncharged statx before the charged one: the budget test passed (exit 0), confirming that limit experimentally. This is a coverage limit, not a product bug; no test was ignored to conceal it. Logs and mutation reproduction script are in the canonical report's library.

## Assumptions and limits

- Directory candidates are discovered and owners checked at runtime; if real-root CI owns all candidates, the test creates a foreign-owned directory only inside its own temporary fixture. Failure to obtain any foreign owner is a failing precondition, never a silent skip.
- Namespace setup is the sole permitted skip. The child writes an entry marker before any mount; mount/scan/assertion failures after that marker always fail. A failed uid/gid map is namespace-creation failure. Missing tools fail. `set -eux` command diagnostics and captured standard error appear in assertion messages. Reviewed twice against local unshare(1) and mount(8): map-root-user, private propagation, tmpfs syntax, bind syntax, and -n to avoid mount-table writes.
- No sudo, loop devices, LVM, filesystem-formatting commands, services, host mounts or writes under protected system directories. Mount commands occur only after entry to the isolated user/mount namespace; here that entry was refused.
- No new privileged-only tests are needed for these regressions. Existing privileged tests are compiled by the required check; privileged runtime remains pending VM.
- Cargo operations run offline using a private copy of the host cache and pinned Rust 1.98.0. No Cargo file changed. Every long command uses timeout 1500.
- No memory update receipts were pending at the checked checkpoint; memory projections were not edited.

## Final validation

The initial Clippy invocation found two non-octal permission literals in the new test; these were corrected. Review also corrected scan to use the same DAC-dropping helper as the other metadata commands. Explicit fixture/config permissions remove umask dependence. An initial wrapper log-path error prevented its CI invocation; the final wrapper ran CI successfully. No check timed out.

| Required check | Result | Evidence |
|---|---|---|
| cargo fmt --check | 0 on final source | latest-fmt.log / latest-fmt.exit |
| cargo clippy --offline --workspace --all-targets -- -D warnings | 0 on final source | latest-clippy.log / latest-clippy.exit |
| cargo test --offline --workspace | 0; 492 passed, 14 existing ignored | final-test.log / final-test.exit; final-ci.log repeats the complete suite on final source |
| cargo build --offline --workspace --features privileged --tests | 0; compilation only | final-privileged.log / final-privileged.exit; final-ci.log repeats the build on final source |
| CARGO_NET_OFFLINE=true tools/ci.sh | 0; source checks, fmt, Clippy, workspace tests and privileged build pass | final-ci.log / final-ci.exit |
| git diff --check | 0 | scope checked again before commit/submission |

Before/after: base registered tests 498 (derived from final 506 minus the eight added tests; no separate base checkout run). Final reported results are 492 passed + 14 ignored = 506 registered. Added targets were absent before; after: FS rules 6 tests, CLI rules 2 tests. Seven new test bodies execute here; the namespace test reports success after the explicitly permitted one-line skip. No new ignore attributes were added. Its local diagnostic was `unshare: write failed /proc/self/uid_map: Operation not permitted`. The foreign-directory test selected /usr, owner uid 65534; this process is uid 0 with CapEff 0, and the asserted fallback count is one.

CI reports 140 owner tags and a passing forbidden-symbol scan. cargo-deny is not installed, and CI explicitly skips its audit: no dependency-policy audit is claimed. Existing privileged runtime remains pending VM. The final CLI test binary was built after the last source edit and exercised in the final CI run. Evidence remains a candidate for coordinator review, not independently verified task success.

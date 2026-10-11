# W0-gate-fix evidence candidate

Implemented B1, S1, S2 and N1 only. No manifests, lockfile, contracts, handoff files or other crates changed. Independent verification and integration remain pending.

## B1 — confined fixture cleanup

Cleanup retains the owner directory handle and original handles for explicitly mode-controlled directories. Saved paths are relative to that owner, sorted shallowest-first. Every component is opened relative to the preceding handle with `O_PATH | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`. Missing, symlink and non-directory components are skipped; final device/inode identity must match the original directory. Permissions are changed through the original verified handle, never a pathname. Keeping original readable handles permits restoration of mode-000 directories. Handle storage is bounded by the existing 250,000-entry fixture cap; fd exhaustion returns a constructor I/O error before restrictive modes are applied.

Regression tests `cleanup_skips_final_symlink` and `cleanup_skips_intermediate_symlink` substitute links to a victim in a separate TempDir and assert victim and child modes remain 0500 after drop. `outside_cleanup_skips_symlinks` repeats both cases for the outside sibling. `cleanup_skips_missing_and_replaced_entries` checks missing entries, regular files and replacement directories (retaining a handle to inspect their unchanged mode after removal). `cleanup_restores_inaccessible_parents_first` checks nested mode-000 removal. `random_tree_removal_does_not_follow_symlinks` checks a separately owned sentinel survives removal.

Checked random-tree cleanup: it returns the same Fixture and has no additional destructor. Checked outside entries: they share this cleanup, anchored to the owner rather than public mutable root/outside fields. Checked removal: TempDir uses std::fs::remove_dir_all, which uses fd-relative no-follow removal on this Linux target; the regressions also assert owner storage disappears and victims survive. Checked LoopXfs/LoopVg cleanup: neither restores permissions; both validate owned mountpoint paths, unmount/detach owned resources and close their TempDir only after successful cleanup. No privileged operations were executed or changed.

The task text said rustix was unattached; the actual baseline already attaches rustix to test-support and uses it for fixture creation. This fix uses that existing dependency, adds no dependency and introduces no unsafe code.

## S1 — shared component type

Added the documented core ComponentState re-export while retaining the W4-B header. `component_state_is_the_core_type` compares TypeId through both public paths. No duplicate struct was introduced.

## S2 — command exit statuses

Both command-dispatch paths now use one conversion that downcasts anyhow::Error to CliError before falling back to Runtime. `command_errors_preserve_exit_codes_through_context` checks codes 2–6 through anyhow with and without context, plus plain runtime errors retaining code 1 in both forms. The check covers the shared dispatch conversion; future command implementations remain stubs.

## N1 — enforced public documentation

Documented Uniform.min and Uniform.max as inclusive byte bounds and enabled missing_docs warnings in test-support. No other missing public documentation was reported, including the privileged-feature documentation build.

## Validation and limitations

All required local commands passed using Rust 1.98.0, the host cargo cache, and offline mode: cargo fmt --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo build --workspace --features privileged --tests; RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps; tools/ci.sh. Also checked privileged-feature test-support documentation with warnings denied and git diff --check. Transcript: attempt output library/validation.log.

Privileged runtime tests remain pending VM. The existing scale test remains ignored by default. CI reports cargo-deny unavailable and skips that audit. Rocky Linux container/VM acceptance and overall Wave 0 readiness are not claimed. B2/N2, S3 and dependency attachments were untouched.

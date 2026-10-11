# Handoff — W1-gate-fix-walk

Implemented G3, G4 and G8 within owned paths; evidence candidate awaiting independent review. No dependencies or shared module/manifests changed.

G3: per-directory heap retains at most top_files single-link top-N candidates ordered by disk descending/inode ascending. The worker size-only cutoff still permits cross-directory ties. Threshold candidates have a separate 4,096 cap; overflow aborts instead of truncating (SCR-001). Tests: same-directory inode winners, reverse-created directory ties, wide_equal_directories_with_slow_consumer (six 512-entry directories, capacity 1, 1/4 workers), threshold_overflow_refuses_without_truncation.

Candidate memory formula: T=threads, C=output capacity, N=top_files, L=max name allocation, H=4096, S=size_of(NotableFile), J=size_of((Reverse<u64>,u64,usize)). With geometric Vec/heap capacity allowance, candidate allocations are bounded conservatively by `(T+C)*(2*(N+H+4)*S+(N+H)*L) + T*(2*(N+4)*S+N*L+2*(N+4)*J+2*(N+4)*8) + T*(S+L)`. This includes queued/active record vectors, temporary directory selection vectors/heaps, worker cutoff heaps and a temporary candidate; allocator bookkeeping is additional per allocation. L=255 for ordinary Linux entry names. Total walker memory additionally includes existing bounded links/skips/children (million entries per active/queued record), errors (10,000 total), traversal tasks (65,536 and 64 MiB path bytes), mount table, record headers and T*getdents_buf. This is a candidate-storage bound, not a claim that the whole walker fits a configured scan memory budget. The orchestrator must budget these other classes too.

G4: decode boundary fields independently; malformed later fields yield an empty-facts boundary and warning. Undecodable boundary fields reject the entire table, conservatively including invalid IDs/device. Existing unavailable-mount-table/no-kernel-mount-ID refusal remains unchanged. Regression malformed_records_preserve_neighbors now verifies the missing-final-field boundary and invalid target failures. nsfs regression remains unchanged.

G8: exported display_lossy_bytes contains the original implementation; both raw types delegate and mount uses a crate-private alias (also preserving probe's existing call site). three_display_entry_points_agree covers printable Unicode, Unicode controls, invalid/incomplete UTF-8, newline and backslash; prior escaping tests remain unchanged.

Validation results will be recorded below. Privileged runtime tests and genuine EL8 capture evidence are pending VM; no privileged commands run.

## Validation

All required available checks exited 0 using Rust 1.98.0, host CARGO_HOME=/home/brewerm/.cargo, RUSTUP_HOME=/home/brewerm/.rustup, CARGO_NET_OFFLINE=true:

- cargo fmt --check
- cargo clippy --offline --workspace --all-targets -- -D warnings
- cargo test --offline --workspace (new regressions all passed; scale tests remain ignored)
- cargo build --offline --workspace --features privileged --tests
- tools/ci.sh (source checks and available checks passed; cargo-deny unavailable and skipped)
- git diff --check

No unrelated workspace failures observed. Feature-enabled compilation passed; runtime mount/VM tests remain pending VM. RSS was not measured by the new ordinary regression: it asserts the exact candidate-count bound under backpressure, as the assigned task requests. Independent verification and SCR downstream decision remain outstanding.

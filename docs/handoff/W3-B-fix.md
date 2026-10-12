# Handoff — W3-B-fix: crossing caps and bounded selection

- Attempt: `attempt-5c5adfda6dde8765008584de2bc40730a2265383fb3678639f74a1d6a026d16d`
- Branch: `hp-5c5adfda6dde8765008584de2bc40730a2265383fb3678639f74a1d6a026d16d`
- Date: 2026-10-11
- Status: implementation evidence candidate; independent verification pending.

## Findings

| Finding | Status | Implementation and regression evidence |
|---|---|---|
| 1: cap charged before eviction | Fixed now | `crates/tidemark-store/src/links.rs::retain_crossing`, called by both `crossing_sets` and `link_report`, checks retained samples minus displaced samples plus incoming samples, then releases the displaced payload before allocating vectors. Unit `crossing_replacement_at_sample_cap` exercises replacement at exactly 1,000,000 samples and genuine overflow rejection without corrupting accounting. Integration `crossing_sets_replaces_at_exact_sample_cap` constructs the review's 1,001-set / 1,000-region snapshot and checks all totals and the retained endpoints. |
| 2: repeated full-vector sorting | Fixed now | All three selectors in `links.rs` use `BinaryHeap<Ranked<T>>`, worst result at the root, bounded admission/eviction, and one final `into_sorted_vec`. `largest_supported_limit_has_logarithmic_selection_work` feeds two million increasing-size candidates with top=1,000,000 through the production heap and comparator, including one million replacements. It asserts explicit comparison bounds proportional to n log k. `increasing_stream_selection_preserves_order_at_large_limits` checks both query entry points and largest results for top=0/25/5,000/1,000,000 against descending synthetic inode/size expectations. |

Both findings were reproduced by source inspection before edits; neither was already fixed. Ranking remains disk descending, LCA raw path bytes ascending, inode ascending. Totals, samples, public signatures and bounds remain unchanged. The unit work counter is thread-local and compiled only under cfg(test).

## Shared-file edits and deviations

None. Only `crates/tidemark-store/src/links.rs`, `crates/tidemark-store/tests/w3b.rs` and this note changed. No new dependencies, unsafe code, metadata I/O or contract changes.

## Measurements

The maximum-limit unit test observed **22,951,423 selection comparisons**, **57,892,686 including final ordering**. Its bounds are 4 n ceil(log2(k+1)) and 6 n ceil(log2(k+1)), respectively, for n=2,000,000 and k=1,000,000. It checks the exact ordered retained million inode IDs. This is algorithmic work evidence, not a wall-clock assertion.

## Validation

Commands use pinned Rust 1.98.0, CARGO_HOME=/tmp/w3b-cargo (private copy of the host registry cache), CARGO_TARGET_DIR=/tmp/w3b-target, and CARGO_NET_OFFLINE=true. Direct Cargo build/test/Clippy commands also use --offline.

| Check | Local result | Evidence under canonical output library/ |
|---|---|---|
| cargo fmt --check | Exit 0 | fmt.log / fmt.exit |
| cargo clippy --offline --workspace --all-targets -- -D warnings | Exit 0 | clippy.log / clippy.exit |
| cargo test --offline --workspace | Exit 0; 476 passed, 0 failed, 13 ignored scale tests | test.log / test.exit |
| cargo build --offline --workspace --features privileged --tests | Exit 0; compile only | privileged.log / privileged.exit |
| tools/ci.sh | Exit 0; source policy, fmt, Clippy and 476 tests/doctests pass; cargo-deny unavailable and skipped | ci.log / ci.exit |
| cargo test --offline -p tidemark-store | Exit 0; 54 tests/doctests passed, 5 scale tests ignored | store-tests.log / store-tests.exit |
| Three crates' --test w3b suites | Exit 0; 16 passed, 2 scale tests ignored | owned-tests.log / owned-tests.exit |
| Store all-targets Clippy with -D warnings | Exit 0 | store-clippy.log / store-clippy.exit |
| Three crates' --features privileged --test w3b build | Exit 0; compile only | owned-privileged.log / owned-privileged.exit |

## Million-set timings

Synthetic complete two-instance sets, boundary depth 2, default top=25; setup is excluded. Visible hardware: Intel Core i7-8750H @ 2.20 GHz, x86_64, 12 logical CPUs. These are local shared-host measurements, not VM results or general guarantees.

| Measurement | Time | Command and evidence |
|---|---|---|
| Library report, release | **1.647570297 s**, assertion passed | cargo test --offline --release -p tidemark-store --test w3b million_set_report_timing -- --ignored --nocapture; scale-library.log / scale-library.exit |
| Full CLI, release | **1.668290403 s**, assertion passed | cargo test --offline --release -p tidemark-cli --test w3b million_set_cli_timing -- --ignored --nocapture; scale-cli.log / scale-cli.exit |

The CLI interval includes launch, query, rendering, output collection and JSON decoding. Both scale tests explicitly count all one million crossing sets. No debug timing was rerun.

The workspace build limitations described by the prior review are absent at this attempt's baseline; all required checks above completed without out-of-scope edits. Cargo-deny's audit remains unperformed because the tool is unavailable.

## Needs from coordinator

No requested contract or shared-file changes. Independent review and integration remain with the coordinator.

## Limits and reviewer notes

The report is local evidence, not task verification or release approval. No privileged runtime tests or VM commands were executed. Existing W3-B mount/cross-filesystem tests remain pending VM. No general deep-incomplete-set, whole-process RSS or O_NOATIME fallback performance claim is made; those paths are unchanged. The shared cap helper is tested directly for both callers; the full cap snapshot runs through crossing_sets, avoiding the report's separate region-pair aggregation cost.

Public rustdoc and owner headers are retained. No unexplained TODOs or production unwrap/expect calls were added.

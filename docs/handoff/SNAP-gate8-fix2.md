# SNAP-gate8-fix2 — review13 fixes (B1, S1, S2, N1)

## Needs from coordinator

- Run the 2M-directory snapbench gate on the host (not run here, as instructed).
- Release-profile store tests cannot run in this sandbox (`/` is owned by uid 65534, strict ancestry refuses it); all
  store targets were run in the debug profile, as in SNAP-gate8-fix. Please run the release suites on the host.
- `docs/handoff/SNAP-gate8-fix.md` (out of my scope) says `children` "still decodes every sibling". That was untrue
  after SNAP-gate8-fix and is true again with this change; treat this file as the correction.

## Findings → change → test

Every new test was confirmed to fail with `reader_v2.rs` reverted to the base (S2 test reported 9 chunk decodes).

- **B1** — `authoritative_root` now also requires the shallow `name` to equal the chunk row's name byte-for-byte and
  validates it (`RawPath::new` for id 0, `RawName::new` otherwise) → `Invalid`. Test:
  `tests/reader_v2.rs::review13_b1_requested_shallow_root_name_must_match_the_chunk` (id 1 renamed `a/b`, empty, `/`,
  `.`, `..`, `x\0y`, a valid-but-different name → `subtree_dirs(1,0,Name,100)` Invalid; id 0 renamed `x\0y` and
  `/other` → `subtree_dirs(0,0,..)` and `metrics_candidates(0,100)` Invalid).
- **S1** — `children()` for a shallow parent now decodes every sibling in one pass (`visit_shallow_children`, as
  `children_count`) and ranks with `TopN`; the SQL `LIMIT` path is gone. Only `children_page` keeps the declared
  narrowing. Test: `review13_s1_children_rejects_a_bad_unreturned_sibling` (`own_charged='x'` on the last child →
  Invalid for limits 1, 2, 100).
- **S2** — (a) `scan` looks chunks up in the LRU (`Backend::cached`, lookup only, no reorder, never inserts) and
  iterates cached rows via the new `ChunkDir::view()` (chunk.rs) instead of re-reading/decompressing; (b) follows from
  (a), since `raw` caches the root chunk; (c) `metrics_candidates` makes one pass feeding both the Disk and
  Reclaimable `TopN`s (one `Walk`; count is no longer halved). Test:
  `reader_v2::child_cursor_tests::deep_queries_decode_each_chunk_at_most_once_and_reuse_the_cache` — balanced 121
  dirs, `plain_depth=0`, 16 dirs/chunk: fresh `subtree_dirs(0,4,Name,1000)` and fresh `metrics_candidates(4,1000)`
  each ≤ 8 chunks (ordinary + streamed); measured 8 each (was 9 and 17). `deep_subtree_scan_streams_each_chunk_once`
  updated to `scanned + decodes == 8`.
- **N1** — `children()` builds `TopN::new(limit)` (which runs `sql_limit`) before the missing-parent early return.
  Test: `review13_n1_missing_parent_still_validates_the_limit` (plain depth 0 and 3, `usize::MAX` → `Integer`, same
  text as v1; limit 3 on a missing parent still `Ok([])`).

## S2 decision (coordinator)

A repeated deep query may still decode chunks the 8 MiB LRU no longer holds; the scan deliberately does not populate
the cache.

## Counts / checks (debug profile)

lib 54 passed/1 ignored; `reader_v2` 42/1; diff_walk 10/2; writer_v2 14; cold 25; storage 33/1; state_trust 4;
snapbench example 2; strict-ancestry `strict_feature_tests` 2. `cargo fmt --check`, clippy `-D warnings` and
`cargo doc -D warnings` clean. Not run: w3a/w3b/w3c/locks/crash targets, release profile, snapbench 2M.
Edits outside the reader: `chunk.rs` gains `ChunkDir::view()` only. No new crates.

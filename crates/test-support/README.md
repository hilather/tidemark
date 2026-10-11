# Test support (W0-C)

Use `materialize_file` for checked-in TOML fixtures, `materialize` for inline
fixtures, and `try_materialize` when an error is expected. A `Fixture` owns one
temporary directory containing sibling `r/` and `outside/` trees; dropping it
restores locked directory permissions and removes both trees. Regular data is
written in 64 KiB chunks; sparse entries use `set_len`. Paths are relative byte
paths, with `\xNN` decoding after TOML parsing. Absolute paths, dot components,
NUL, duplicate names, symlink parents and cyclic hardlink targets are rejected.
Hardlink targets are relative to `r/`, including those in `[[outside]]`.
Symlink targets are raw bytes and deliberately may point outside or form loops.

`wide_dir`, `deep` and `hardlink_web` use `generate` profiles instead of huge
TOML lists. `mixed_special.toml` covers FIFO and socket entries; its root-only
extension is `mixed_special_root.toml`, exercised only in the privileged suite.
Socket creation may be denied by worker sandboxes; the ordinary integration
test reports this skip, and the VM smoke test requires success.

`model_tree(seed, params)` and `random_tree(seed, params)` share deterministic
structure. Counts mean directories including root and distinct file inodes;
link sets use 2..=max_links_per_set discovered entries and probabilistically one
outside link. Symlink ratio applies to unlinked inodes. Model file allocation
rounds to synthetic 512-byte blocks and timestamps use a fixed injected epoch;
real fixtures have filesystem-dependent allocation and creation timestamps.
The public model fields let tests inject directory blocks, error records,
negative mtimes, repeated links in one directory and nlink races. Valid models
have parents preceding children, unique sibling names and at most one million
combined items. Conversion yields the frozen core `DirRecord` type, sorted
entry names, named link refs and all single-link notable candidates.

`reference::compute` uses independent subtree and inode sets, never aggregation
code or `to_dir_records`. It includes directory inode bytes in all applicable
metrics and age buckets. `golden::LINK_SEMANTICS_BASIC` contains the exact 03 §6
table and `golden::link_semantics_basic()` produces the abstract tree. For disk
comparisons, measure directory blocks at test time; apparent comparisons also
measure directory st_size. The GNU du directory-apparent probe addresses
SCR-006 without changing Tidemark's metric definition.

Call `oracle::gnu_available("du")` / `gnu_available("find")` before tests using
these oracles; missing/non-GNU tools print a reason and the caller skips the
comparison. The fallible variants report process and parse errors. Each du
call is separate, so hard links are deduplicated within one subtree invocation.
Find output contains types and inode ids, never filenames. Mount/exclusion
adjustments belong to the caller; these fixtures do not contain mounts.

`xfs` and `lvm` are compiled only with `privileged`. Run their tests only through
`cargo xtask vm-test` on a disposable Rocky VM. Constructors require root;
cleanup is explicit and testable, with Drop as a rollback fallback. Cleanup
stops on error and preserves temporary storage if mount/loop removal fails.
Test VG names are immutable, unique, and prefixed `tmtest_`; suffixes cannot
inject command options. No helper operates on a pre-existing VG.

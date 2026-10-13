# Wave 0 — Foundation and frozen contracts

**Goal:** a compiling workspace in which every later task has a stub file to fill in, and the shared contracts (types, config, CLI surface, test fixtures) are written once and frozen.

**Agents:** 1, working through W0-A → W0-B → W0-C in order. W0 is sequential on purpose: contracts written by one author are consistent. W0-A creates the empty crates and tooling; **W0-B writes `tidemark-core` first and then the cross-task stubs**, which need the core types.

**Entry criteria:** the repo is initialised with `docs/plan/` (this plan) and `AGENTS.md` at the root (`10-agent-playbook.md` §2).

**Exit gate:** see the end of this file. **After this gate, `tidemark-core` is frozen** (changes go through spec-change requests).

| Task | Title | Owned paths |
|------|-------|-------------|
| W0-A | Workspace scaffold, tooling, stubs | `Cargo.toml`, `rust-toolchain.toml`, `.cargo/`, `deny.toml`, `.gitignore`, `xtask/**`, all `crates/*/Cargo.toml`, all stub files, `packaging/README.md`, `docs/handoff/.keep` |
| W0-B | Core contracts and CLI surface | `crates/tidemark-core/**`, `crates/tidemark-cli/src/{main.rs,cli.rs}`, `crates/tidemark-cli/src/cmd/**` (as stubs with final `Args`), `crates/tidemarkd/src/main.rs` (stub) |
| W0-C | Test support: fixtures, oracles, loop-XFS helper | `crates/test-support/**`, `xtask/src/vm_test.rs` |

---

## W0-A — Workspace scaffold and tooling

**Read first:** `02-architecture.md` §4, `10-agent-playbook.md` §3–5, `AGENTS.md`.

### Deliverables

1. **Workspace `Cargo.toml`** with members:
   `crates/tidemark-core`, `tidemark-sys`, `tidemark-fs`, `tidemark-scan`, `tidemark-store`, `tidemark-live`, `tidemark-cli`, `tidemarkd`, `test-support`, and `xtask`.
   - `[workspace.package]`: version `0.1.0`, edition `2021`, license (team's choice), `rust-version` = pinned toolchain.
   - `[workspace.dependencies]` declares every third-party crate from `02-architecture.md` §11, plus `globset`, `smallvec`, `thiserror`, `anyhow`, `clap_complete` and `clap_mangen`, so member crates use `dep.workspace = true`.
   - `[workspace.lints]`: `rust.unsafe_code = "forbid"`, `clippy.all = "warn"`, `clippy.unwrap_used = "warn"`, `clippy.expect_used = "warn"`. Every crate uses `lints.workspace = true` **except `tidemark-sys`**, which has its own `[lints]` table (`unsafe_code = "deny"`, `unsafe_op_in_unsafe_fn = "deny"`, same clippy set) so its modules can `#[allow(unsafe_code)]`. A `forbid` can't be relaxed, and workspace lints can't be overridden per lint.
   - `clippy.toml`: `allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`.
   - `[profile.release]`: `lto = "thin"`, `codegen-units = 1`, `debug = "line-tables-only"` (for useful backtraces and perf), `panic = "unwind"`.
2. **`rust-toolchain.toml`** pinning the then-current stable Rust (record the version in the handoff), with `rustfmt` and `clippy` components.
3. **Crate skeletons** with the module files listed in the table below. Every file compiles, and each starts with a doc comment `//! Owned by: W<task>` plus a one-line purpose. **`tidemark-cli` and `tidemarkd` are lib + bin crates** (`src/lib.rs` holds everything; `src/main.rs` is a thin `fn main`), so `tidemarkd::firstboot`, `tidemark_cli::client` and their parsers are reachable from tests and fuzz targets.
   - **(Done in W0-B, after `tidemark-core` exists.)** Stub every public signature given in the wave files: `wave-1-primitives.md` (W1-A … W1-E), plus `tidemark_sys::fanotify::{probe, FanotifyCaps}` (W5-A), `tidemark_fs::lvm::{collect, LvmReport}` (W4-A) and `tidemark_live::rates::{slope_bytes_per_sec, time_to_full}` (W5-D). Cross-task calls then compile from day one. For W4-A, stub the **whole** `tidemark_fs::lvm` interface with final field lists, not just `collect`.
   - Tidemark is strictly read-only (ADR-008): no crate may contain code that changes file metadata, mounts, LVM or boot configuration. `xtask ci` scans the **non-test library and binary sources** (excluding `test-support/`, `xtask/`, `tests/`, `#[cfg(test)]` modules and string literals) for forbidden symbols: `FS_IOC_FSSETXATTR`, `Q_SETQUOTA`/`Q_XSETQLIM`, `libc::mount`/`umount`, and `Command::new` with an `lv*`/`vg*`/`pv*` command other than `lvs`/`vgs`. Any hit fails CI.
   - Stubs whose documented signatures can return an error return `Err(NotYetImplemented)` (an `io::ErrorKind::Unsupported` error, or the crate's error type with a `NotYetImplemented` variant), never `unimplemented!()`, so callers can treat them as "unknown". Error-free signatures (plain values, `Option`, references, constructors or unit) keep their exact signature and return inert placeholders: `None`, an empty collection, zero or a default value. The wave-file signature wins over this generic rule. Callers treat placeholders as "unknown", never real data, until the owning task replaces the body without changing the signature (SCR-005, `13`).
   - Later waves' internal modules may use `unimplemented!("W<task>")`.
4. **`tidemark-sys`** gets its lints from its own `[lints]` table (above). Unsafe is allowed per module with `#![allow(unsafe_code)]`. All other crates inherit `forbid`.
5. **`xtask`** (binary crate, `cargo xtask <cmd>` via `.cargo/config.toml` alias):
   - `ci`: runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check` (if installed; warn otherwise), the owner-tag check and the forbidden-symbol scan.
   - **Features:** every member crate declares `privileged = []` (it may be empty), so `--workspace --features privileged` works.
   - `vm-test`: stub here, implemented in W0-C. `gen-fixture`, `bench`: stubs owned by W7-B. `rpm`: stub owned by W6-C.
6. **`deny.toml`**: license allow-list and advisories on; deny `openssl-sys`, since nothing should need it (keep the binary free of system TLS dependencies; no HTTP/TLS stack remains in v1 (ADR-020)).
7. A `.gitignore` covering `target/` and `*.db.tmp`.

### Stub map (file → owning task)

| Crate | Files (owning task) |
|-------|---------------------|
| tidemark-sys | `xfs.rs` (W1-C) · `ioprio.rs` (W1-D) · `fanotify.rs` (W5-A) |
| tidemark-fs | `mount.rs` `blockdev.rs` `probe.rs` `canon.rs` (W1-A) · `lvm.rs` (W4-A) · `walk/mod.rs` `walk/worker.rs` `walk/getdents.rs` (W1-B) · `throttle.rs` `procfs.rs` `attribution.rs` (W1-D) · `symlinks.rs` (W3-B) |
| tidemark-store | `writer.rs` `reader.rs` `catalog.rs` `timeseries.rs` `retention.rs` `space_guard.rs` (W1-E) · `diff.rs` (W3-A) · `links.rs` (W3-B) · `export_ncdu.rs` (W3-C) |
| tidemark-scan | `agg/mod.rs` `agg/linkset.rs` `agg/lca.rs` `agg/rollup.rs` `agg/notable.rs` (W2-A) · `bulk/mod.rs` `bulk/join.rs` `bulk/names.rs` (W2-B) · `orchestrator.rs` `mode.rs` `progress.rs` (W2-C) |
| tidemark-live | `watch/mod.rs` `watch/fdmode.rs` `watch/fidmode.rs` `watch/rings.rs` (W5-A) · `procscan.rs` (W5-B) · `rates.rs` `alerts/mod.rs` `alerts/rules.rs` `alerts/deliver.rs` `now.rs` (W5-D) |
| tidemark-cli | see W0-B |
| tidemarkd | `main.rs`, `lib.rs` (W0-B stub → W6-A) · `firstboot.rs` (W4-B) · `api.rs` `state.rs` `scheduler.rs` `services/{sampler,lvm,watcher,procscan,tswriter,alerts}.rs` (W6-A) · `metrics.rs` (W6-B) |

### Acceptance

- [ ] `cargo build --workspace` and `cargo xtask ci` pass on a clean checkout.
- [ ] `cargo build` inside a `rockylinux:8` container succeeds (command recorded in the handoff note).
- [ ] Every stub file names its owner task.

---

## W0-B — Core contracts and CLI surface

**Read first:** `03-link-semantics.md`, `04-scanner.md` §1, `05-storage.md` §1–3 and §6, `07-live-monitoring.md` §7 and §7a, `08-interfaces.md` (all).

### Deliverables in `tidemark-core`

| Module | Content |
|--------|---------|
| `path` | `RawName` (non-empty, no `/`) and `RawPath` (absolute, normalised, no trailing slash except `/`), backed by bytes. Methods: `from_path`, `as_path`, `join`, `parent`, `file_name`, `starts_with` (component-wise), `display_lossy` (escapes control chars and invalid UTF-8 as `\xNN`), `hash64` (xxh3, as `i64`), `to_json` (`{"path": …}` or `{"path_b64": …}`). Implements `Ord` by bytes. SCR-007 adds `pub fn display_lossy_bytes(bytes: &[u8]) -> String`: printable Unicode is kept; control characters, invalid UTF-8 bytes and backslashes use uppercase `\xNN`. Both `display_lossy` methods delegate to it with unchanged behaviour. |
| `units` | `ByteSize` (serde from `"1GiB"` or an integer), `fmt_bytes(u64)` (exact bytes below 1 KiB; otherwise binary units, 3 significant digits after rounding and unit promotion: `1.23 GiB`, `611 GiB`), `parse_rate("50GiB/h") -> Result<f64, CoreError>` (bytes/s), `fmt_rate`. |
| `time` | `AGE_BOUNDS_SECS = [86_400, 604_800, 2_592_000, 7_776_000, 31_536_000]`, `age_bucket(scan_start, mtime) -> usize` (future mtime → 0), `ScanId::new(now: i64) -> Result<ScanId, CoreError>` (`YYYYMMDDTHHMMSSZ-<8 lowercase hex>`; errors outside the four-digit UTC-year range), `unix_now()`, humantime helpers. |
| `scan` | Exactly the types in `04-scanner.md` §1, including `Gate` and `NoopGate`. Add `SnapshotStatus { Complete, Partial }` and `ScanMode { Walk, Bulk }`. |
| `snapshot` | `SnapshotMeta` (every key in `05` §1, typed), `DirRow` (every `dirs` column), `FileRow`, `LinkSetRow`, `LinkInstanceRow`, `SkipRow`, `ErrorRow`, `AnnotationRow`, `SortKey`, `FileFilter`, `LinkFilter`, `FileHit` (row + optional full path), `LinkSetHit` (row + LCA path + instance count). |
| `live` | Exactly the types in `07` §7a (field lists are normative), plus `NowReport`/`MountNow` from `07` §7. |
| `dev` | `DevKind { Disk, Partition, Md { level }, Dm, Other }`, `BlockDevInfo { name, major, minor, kind, rotational: Option<bool>, scheduler: Option<String> }`, `DiskStat { read_ios, read_merges, read_sectors, read_ticks_ms, write_ios, write_merges, write_sectors, write_ticks_ms, in_flight, io_ticks_ms, time_in_queue_ms }` |
| `config` | The full schema in `08` §3 as serde structs with `#[serde(deny_unknown_fields)]` and defaults matching the doc. Type names: `Config`, `GeneralConfig`, `RootConfig`, `ScanConfig`, `ThrottleConfig`, `ThrottleProfile`, `SamplerConfig`, `LvmConfig`, `WatchConfig`, `ProcscanConfig`, `MetricsConfig`, `AlertsConfig`, `AlertRule`, `RetentionConfig`, `SnapshotRetention`, `SampleRetention`. `Config::load(path)`; `Config::validate() -> Vec<ConfigIssue>` (paths absolute, windows parse, rule kinds valid; whether mounts exist is checked at runtime, not here). Unit tests round-trip the doc's example verbatim. |
| `api` | `Request { v, id, method, params: serde_json::Value }`, `Response { v, id, ok, result?, error? }`, `ApiErrorCode`, and typed param/result structs for every method in `08` §2. |
| `json` | `JsonDoc<T> { schema: Cow<'static, str>, data: T }`, used by every `--json` output. |
| `error` | `CoreError` for config/parse errors (thiserror). |

The exact helper signatures and contracts are frozen in `13-contract-decisions.md` SCR-004; snapshot widths/filters in SCR-001, API DTOs in `08` §2 and config defaults in `08` §3. `GateStats` is defined in core (`scan`) with the W1-D fields. Shared report DTO adapters encode all paths as mutually exclusive `path`/`path_b64`, including nested holders and directory hot entries; test whole envelopes. `JsonDoc` derives owned `Deserialize` without `#[serde(borrow)]`. CLI sort arguments use a wrapper with `Growth` (`08` §1.2), separate from snapshot `SortKey`.

All structs derive `Debug, Clone` and, where sensible, `PartialEq` and `serde::{Serialize, Deserialize}`. Keep `tidemark-core` free of I/O, except `Config::load` reading one file.

### Deliverables in `tidemark-cli`

- `cli.rs`: the complete `clap` derive tree for every command and flag in `08` §1. Each subcommand's `Args` struct lives in its own file, `cmd/<name>.rs` (deploy: `cmd/deploy/{mod,stage,verify,unstage}.rs`), with the final flags. Each `run()` returns `Err(anyhow!("not implemented yet (W<task>)"))` and exit code 1.
- `lib.rs` + thin `main.rs`: config loading, logging setup (`tracing-subscriber`, stderr), `--json`/`--color` handling, exit-code mapping (`08` §1.3) via a `CliError` enum.
- `cmd/completions.rs`: fully implemented (`clap_complete`).
- `render/table.rs` and `render/size.rs`: minimal shared helpers (aligned columns, ASCII fallback, `fmt_bytes`, lossy paths), fully implemented. Later tasks may **add** new `render/<name>.rs` files they own, but must not edit these two (request changes via SCR).

### Cross-task stubs (after `tidemark-core` is written)

Create the stub signatures described in W0-A item 3 in their owners' files. Stubs follow the error-capable/error-free rules in W0-A item 3 (SCR-005, `13`); their types use the real `tidemark-core` types, so callers compile against final signatures.

Owner map for the command files: `probe` W1-A · `scan` `snapshots` `top` `files` W2-C · `diff` W3-A · `links` `symlinks` W3-B · `export` `wide` W3-C · `lvm` W4-A · `deploy/*` W4-B · `watch` W5-A · `phantom` `writers` W5-B · `now` `hot` `alerts` `status` W6-A · `tui` W7-A.

### Deliverables in `tidemarkd`

`main.rs` stub that parses `--config` and `--check-config` (fully implemented: load + validate + print issues, exit 2 on errors) and otherwise exits with "not implemented (W6-A)".

### Acceptance

- [ ] `tidemark --help` lists every command in `08` §1.2; each `--help` shows its flags.
- [ ] `tidemarkd --check-config` accepts the example config in `08` §3 verbatim and rejects an unknown key with a helpful message.
- [ ] Unit tests: `RawPath` normalisation and `starts_with` component semantics (`/data/rep` does **not** start with `/data/re`); non-UTF-8 round-trip; `display_lossy` escaping; `age_bucket` boundaries (exact boundary values belong to the older bucket: an age of exactly 86,400 s is bucket 1); `ByteSize`/`parse_rate` parsing; `ScanId` format.
- [ ] `cargo doc --workspace --no-deps` builds without warnings.

---

## W0-C — Test support: fixtures, oracles, loop-XFS helper

**Read first:** `09-testing.md` §1–5, `03-link-semantics.md` §6.

### Deliverables in `test-support`

1. **Fixture DSL** (`fixture.rs`): parse TOML as in `09` §2. Support `file` (`size` bytes written as real data in 64 KiB chunks; `sparse` via `set_len`), `dir` (with `mode`), `hardlink`, `symlink`, `fifo`, plus `[[outside]]` entries created in a sibling directory outside the root. Names may contain `\xNN` escapes, decoded to raw bytes. `materialize(spec) -> Fixture` and `materialize_file(path)`.
2. **Random trees** (`random.rs`): `random_tree(seed, RandomTreeParams { dirs, files, max_depth, link_sets, max_links_per_set, incomplete_ratio, symlink_ratio, size_dist })`. Deterministic per seed. Also `model_tree(seed, params) -> ModelTree`: the same structure as an in-memory model **without touching disk**, for aggregator property tests (W2-A), plus `ModelTree::to_dir_records()` producing `Vec<DirRecord>` as a walk-mode scanner would.
3. **Brute-force reference** (`reference.rs`): given a `ModelTree`, compute every metric in `03` §3 per directory using sets of inode IDs. Slow and obviously correct. This is the oracle for W2-A's property tests.
4. **Oracles** (`oracle.rs`): `du_disk(path) -> u64` (`du -sxB1`), `du_apparent(path)`, `find_counts(path) -> {files, dirs, inodes}`. Skip with an explanatory message when GNU tools are missing.
5. **Loop XFS** (`xfs.rs`, compiled only with feature `privileged`): `LoopXfs::new(size, mkfs_opts: &[&str], mount_opts: &str) -> LoopXfs { image, mountpoint }`, with unmount, loop detach and file removal on `Drop`. Panics with a clear message if not root.
6. **Loop LVM** (`lvm.rs`, feature `privileged`): `LoopVg::new(name_suffix, pv_size, layout: LvLayout { linear: Option<size>, thin_pool: Option<(pool_size, thin_lv_virtual_size)> })`. It creates loop PVs, a VG named `tmtest_<suffix>`, the LVs, `mkfs.xfs` and mounts. On `Drop` it unmounts, removes LVs/VG/PVs and detaches loops, and it refuses to operate on any VG whose name lacks the `tmtest_` prefix.
7. **Fixture files:** `fixtures/link_semantics_basic.toml` (exactly `03` §6, with directory sizes noted), plus `wide_dir`, `deep`, `symlink_loops`, `nonutf8`, `hardlink_web`, `sparse`, `mixed_special` as described in `09` §2. Large ones are generated by code rather than listed.
8. **Golden expectations** for `link_semantics_basic` as a Rust table, so W2-A's tests assert the exact numbers in `03` §6, adjusted for directory blocks measured at test time.

### `xtask vm-test`

`cargo xtask vm-test --host <ssh> [--remote-dir DIR] [--filter PATTERN] [--suite cargo|install|fault] [--env NAME=VALUE]... [--nocapture]`:

1. rsync the workspace, excluding `target/`.
2. Over SSH: `cargo build --workspace --features privileged --tests`, then `sudo env "PATH=$PATH" cargo test --workspace --features privileged -- --test-threads=1 [PATTERN]` (EL8 sudo's `secure_path` ignores `-E`, so pass `PATH` explicitly). Privileged tests are gated by the feature only, **not** by `#[ignore]`; scale tests use `#[ignore = "scale"]` and are not run here. `--suite install|fault` runs the script suites in `tests/install/` and `tests/fault/` instead of cargo tests.
3. Stream the output; exit with the remote exit code. `--nocapture` appends `--nocapture` to test binary arguments after `--`.
4. Repeatable `--env NAME=VALUE` accepts only names matching `TIDEMARK_[A-Z0-9_]+`, rejecting others before any connection. Pass pairs through SSH and sudo next to PATH (`sudo env "PATH=$PATH" NAME=VALUE ... cargo test ...`), shell-quoting values so spaces, quotes and metacharacters cannot escape. Unit-test accepted/rejected names, a value with a space, quote and semicolon, and argument positions.
5. Document disposable loop-device md provisioning in `xtask/README.md` and pass `--env TIDEMARK_W1A_MD_DEVICE=/dev/mdX`; commands and required gate evidence are in `09` §5. Run provisioning only in the VM.

Document VM prerequisites in `xtask/README.md`: Rocky 8.8 and 8.10 VMs, packages from `09` §5, a passwordless-sudo test user, and a rustup toolchain.

### Acceptance

- [ ] `materialize(link_semantics_basic)` reproduces the tree. `du_disk` on `/r/a` and `/r` returns the values in `03` §6, plus measured directory blocks (test included).
- [ ] The `ModelTree` reference reproduces `03` §6 exactly (test included).
- [ ] `random_tree` is deterministic: the same seed gives an identical tree listing (test).
- [ ] Privileged smoke tests create and mount a `LoopXfs` and a thin `LoopVg` and write a file to each. Status in the handoff note: run on VM, or pending.

---

## Wave 0 gate checklist

- [ ] `cargo xtask ci` green; the `rockylinux:8` container build is green.
- [ ] A human has reviewed the `tidemark-core` public API against `03`, `04`, `05`, `07` and `08`. **This is the most important review in the project.** Contract mistakes found later cost a spec-change request each.
- [ ] Every stub file exists with the correct owner tag (a script in `xtask ci` checks that each file under `crates/*/src` has an `//! Owned by:` line).
- [ ] `xtask vm-test` runs the privileged smoke test on at least one Rocky 8 VM.
- [ ] Tag `wave-0-done`. Freeze `tidemark-core`.

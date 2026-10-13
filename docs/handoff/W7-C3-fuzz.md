# Handoff — W7-C3-fuzz: parser properties and nightly fuzz targets

## Needs from coordinator

- Fanotify arbitrary-buffer fuzzing needs a separately owned safe, non-descriptor-owning parser seam. `tidemark-sys::fanotify::EventIter` has a private buffer field; `decode` constructs `OwnedFd` from supplied integers. Its only public source is a real fanotify read. Arbitrary input could close unrelated process descriptors, so no unsafe/private-source workaround or placeholder target was added.
- The fdinfo mount-id parser is actually private `tidemark-live::watch::parse_mount_id`. The public `Watcher::mount_identity` reads real procfs data. A separately owned seam/test is needed to inject arbitrary text.
- Daemon has no proptest dev-dependency and no allowed proptest-bearing crate depends on it. Its new integration file therefore enumerates deterministic report mutations rather than using proptest. Add dependency/test access in a separately owned follow-up if exact property parity is required.
- API request deserialization uses the exact public Request type used by daemon. Daemon socket framing and dispatch remain uncovered. The response target additionally exercises public `Client::connect` / `call` with a disposable listener; its smoke properties returned early on listener EPERM in this attempt and need unrestricted host evidence. The GitHub fuzz target asserts that sockets are available.
- Nightly/libfuzzer-sys and GitHub workflow execution remain **unverified until the first GitHub run**. No local fuzz build was attempted. The 24-hour cumulative crash-free release gate remains outstanding.

- **Attempt:** attempt-f18758c04f284b3f02779ba6d1f87f05b75ebe135e841b415a9889282c063f6f
- **Branch:** hp-f18758c04f284b3f02779ba6d1f87f05b75ebe135e841b415a9889282c063f6f
- **Date:** 2026-10-13
- **Status:** evidence candidate with the coverage gaps above; independent verification pending

## What was built

Four new integration suites exercise public parser seams without changing product visibility or main manifests. Filesystem, CLI and store properties use 48 cases each, fixed RNG seed `0x57374333`, and at most 1,024 shrink iterations. Each suite tests arbitrary bytes and structured mutations (truncation, byte flips, duplication, inserted long fields, embedded NUL and invalid UTF-8). Large inputs, huge numeric fields and 512-level JSON/TOML-shaped nesting have two-second elapsed assertions; documented size refusals are also asserted.

The standalone `fuzz/` workspace defines 13 short targets with small seed corpora and shared disposable-state helpers compiled by the stable smoke tests. The GitHub matrix builds all targets and runs each for 600 seconds; failures upload per-target artifacts. Jobs have read-only repository permission, 45-minute deadlines, 1 MiB inputs, 2 GiB RSS and two-second execution limits. Service and API fakes perform no host changes.

## Target coverage

Property names `arbitrary_bytes`, `near_valid`, and `large_and_deep_inputs_finish_within_two_seconds_each` apply to every parser exercised by that suite. `fs`, `CLI`, `daemon`, `store` below denote `crates/<crate>/tests/fuzz_smoke.rs`.

| Function / seam | Product file | Smoke test | Fuzz target | Seed corpus source |
|---|---|---|---|---|
| `parse_mountinfo` | fs `src/mount.rs` | fs properties + size refusal | `mountinfo` | fs `tests/w1a/data/mountinfo` |
| `parse_disk_stat` | fs `src/blockdev.rs` | fs properties + 64 KiB refusal | `disk_stat` | inline eleven-counter format |
| `parse_cgroup_unit` | fs `src/procfs.rs` | fs properties | `cgroup_unit` | fs `tests/w1d/data/cgroup-v2.txt` (v1 also in properties) |
| `parse_fdinfo_flags` | fs `src/procfs.rs` | fs properties | `fdinfo_flags` | fs `tests/w1d/data/fdinfo.txt` |
| `parse_stat_starttime` | fs `src/procfs.rs` | fs properties | `stat_starttime` | fs `tests/w1d/data/stat.txt` |
| `parse_lvs_json` | fs `src/lvm.rs` | fs properties + 16 MiB refusal | `lvs` | test-support `fixtures/lvm/lvs.json` |
| `parse_vgs_json` | fs `src/lvm.rs` | fs properties + 16 MiB refusal | `vgs` | test-support `fixtures/lvm/vgs.json` |
| Request `serde_json::from_slice` | daemon `src/api.rs`, core `src/api.rs` | CLI properties | `api_request` | inline status request format |
| Response serde + public `Client::call` decoder | CLI `src/client.rs`, core `src/api.rs` | CLI properties, `public_client_response_decoder`, `public_client_response_mutations`, `public_client_response_bounds` (socket coverage may be unavailable) | `api_response` | inline client response sample |
| `Config::from_toml_bytes` | core `src/config.rs` | CLI properties + 1 MiB refusal | `config` | `packaging/config/tidemark.toml` |
| `StagedRecord` serde + `deploy::verify` readers | CLI `src/deploy/mod.rs` | CLI properties, `deployment_readers`, `deployment_mutations`, `deployment_reader_bounds` | `staged` (both CLI files read) | inline staged record fields from deploy tests |
| `firstboot::needs_report` + FirstbootReport serde | daemon `src/firstboot.rs` | daemon `mutated_reports_and_large_inputs`; CLI reader properties also cover firstboot JSON Value | `firstboot` | firstboot test report fields |
| `Catalog::record_finish` / `get` stored-path resolution | store `src/catalog.rs` | store properties; raw SQL injects invalid UTF-8, NUL, traversal and legacy absolute paths | `catalog_path` | inline portable snapshot relative path; additional paths in properties |
| `parse_mount_id` | live `src/watch/mod.rs` | inaccessible to arbitrary input | absent, blocked | none |
| fanotify `decode` / EventIter | sys `src/fanotify.rs` | inaccessible safely to arbitrary input | absent, blocked | none |

Every supplied target has `fuzz/corpus/<target>/sample`. Recorded-format fixtures are synthetic; none are claimed real server captures. String parsers receive `String::from_utf8_lossy`, matching their procfs caller boundary; invalid UTF-8 also reaches byte parsers unchanged. Option-returning parsers retain their actual Option contract rather than inventing errors.

## Panic findings

No product panic was observed. No proptest regression files were produced. One initial harness failure unwrapped the expected oversized deployment-record refusal; the harness was corrected to assert the typed refusal. This was a test assertion failure, not a product panic.

## Shared-file edits and dependencies

Only `.gitignore` gained `/fuzz/target/`, `/fuzz/artifacts/`, `/fuzz/coverage/`. No workspace manifest, lockfile, product source or existing test changed. Shared helpers live under owned `fuzz/support/` and are included by the integration suites.

`libfuzzer-sys` is needed only for the standalone nightly fuzzer. Its standalone manifest declares path dependencies on parser owners plus serde_json, tempfile and rusqlite (matching existing workspace versions/features) for public decoding and disposable fixtures. No new main-workspace dependency was introduced. Helper unwrap lint allowances are confined to fixture setup: infrastructure failures must fail a test/fuzz run immediately.

## Assumptions and deviations

- Retained task scope overrides generic branch instructions: used the existing canonical attempt branch, with no branches, merges or pushes.
- Missing proptest dependencies cannot be added here; core/config/API properties were placed in CLI. Daemon remains deterministic-only as described above; sys/live have no safely injectable public byte seam for their requested targets.
- A two-second elapsed assertion provides observed performance evidence, not a hard in-process watchdog or proof of asymptotic complexity. LibFuzzer execution/RSS limits and job/command timeouts provide separate containment. No claim that finite tests prove bounded memory for every possible input.
- All temporary-state writes occur in fresh tempfile directories; no mounts, LVM operations, services or privileged actions were performed. No new privileged tests are appropriate for these pure-parser/disposable-state targets. Existing VM acceptance remains pending VM.
- Toolchain binaries and cache were exposed explicitly through PATH, CARGO_HOME and RUSTUP_HOME because the worker default PATH omitted cargo and its default rustup home was read-only. Offline Cargo was used throughout; no network fetching or fuzz build.

## Validation

The explicit public-client bounds test with `--nocapture` printed `AF_UNIX listener denied (EPERM)`: 96 configured client properties and their bounds case did not reach the transport decoder here. Public Request/Response serde properties did run. The GitHub response target requires listener availability.

All long commands used `timeout 1500`. The final CI run covered the complete submitted smoke suites.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | exit 0; also repeated successfully by final CI |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0; final CI also passed after the client fixture addition |
| `cargo test --offline --workspace --no-fail-fast` | exit 0, 730 passed / 0 failed / 14 ignored in summed result lines; executed before the last three public-client tests were added |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | exit 0 on final code; format, Clippy, source policy, workspace tests and `--features privileged --tests` build passed |
| Dependency audit in CI | **not run**: cargo-deny not installed, allowed warning from existing xtask |
| Standalone fuzz Rust formatting | `rustfmt --check` passed for targets and shared helpers |
| Static fuzz project checks | TOML loaded, all 13 target paths/corpora/matrix entries matched; YAML structure parsed |
| Nightly build / executions | not attempted locally; first GitHub run pending |
| Runtime VM / unrestricted socket acceptance | pending VM/unrestricted host |

Final CI smoke results: fs 4 tests / 0 failures in 0.78 s; CLI 10 / 0 in 1.65 s; store 3 / 0 in 0.53 s; daemon 1 / 0 in 0.23 s. There are 480 configured property cases across ten properties; 96 client transport cases returned early because sockets were denied, leaving 384 parser property cases actually exercised. Daemon's deterministic test makes 412 calls (valid sample, every truncation/byte flip, four oversized buffers and nesting). Each suite finished below ten seconds. All observed large-input elapsed assertions and size refusals passed.

Logs are preserved under the canonical attempt output directory's `library/`: `fuzz-smoke.log`, `fuzz-workspace.log`, `fuzz-ci.log`, `fuzz-clippy.log`, `fuzz-fmt.log`, and `fuzz-socket.log`. No observed product panic or regression file. No privileged tests were executed; the feature-enabled suite only compiled.

## Completion boundary

Owned-path implementation and available local checks are complete as evidence, with the parser-access and environment gaps listed at the top. This note does not assert independent task verification or release-gate success. Final commit, cargo clean, clean git status and submission receipt are recorded in the canonical attempt report.

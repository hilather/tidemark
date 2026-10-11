**NOT READY — gate checkout: 1 BLOCKER, 3 SHOULD-FIX, 1 NIT.**

**Exact blind candidate: 2 BLOCKER, 3 SHOULD-FIX, 2 NIT**, including the two previously reported core findings absent from the newer gate checkout. These are review findings and recommendations, not an acceptance or integration decision.

## Revisions and scope

The supplied instructions identify two different trees. The blind review names exact candidate `51552ac1bdeb2f41740b0200566b4f67b63ca3d5`, base `5fbaa9ab8e267693b0efd5a8f98ef7a2ace12942`, with whole-candidate-tree scope. The assigned gate checkout and report submission base are `c8d7d841872a6795f3e3a01292983ac98352f8cd`. Both were examined and built separately. The candidate was exported under ignored `target/review-candidate/`; the supplied checkout was retained for the gate report.

The entire difference between those source trees is `crates/tidemark-core/src/config.rs`, `crates/tidemark-core/tests/example.toml`, `crates/tidemark-core/tests/review_regressions.rs`, and the added `docs/reviews/W0-B1-fix2.md`. Thus the newer core fixes are **not** credited to the blind candidate. All other source findings below apply to both revisions. Source locations are common to both unless explicitly marked candidate-only.

Read the root instructions, whole Wave 0 plan, Wave 1 interfaces, contract decisions, ADR-018, playbook Definition of Done/gates, every existing handoff/review, and relevant interface/storage/scanner/link/test specifications. The earlier full core review was not repeated; core work here checked R1/R2 and downstream contract seams. Only this report is changed in the repository.

## Findings

### B1 — BLOCKER: fixture cleanup can change permissions outside its owned directory

Reference: `finding:w0-gate-cleanup-confinement`. Location: `crates/test-support/src/fixture.rs:30`. Fix owner: **W0-C**.

`Fixture::drop` restores saved directory modes through `fs::set_permissions(path, 0700)`. That operation follows symlinks. Creation-time validation cannot protect it after a test replaces a tracked directory, or one of its parent directories, with a symlink. Fixture mutation is relevant to the planned walker race tests, and the same destructor runs as root in privileged tests.

An unprivileged compiled reproducer used only two owned `TempDir`s: one fixture and a separate victim. Replacing the final component changed the victim from `0500` to `0700` on fixture drop; replacing an intermediate parent produced the same result. Both victims remained present. This establishes a metadata write outside the fixture, without running any privileged operation or touching a real external directory.

Restore permissions using handles anchored to the fixture owner directory: traverse **every** component with fd-relative directory opens and no-follow semantics, then change the verified directory handle's mode. Retain shallowest-first restoration so inaccessible descendants can be reached; skip missing, replaced or non-directory entries. A final-component `symlink_metadata` check alone leaves ancestor traversal and replacement races unresolved. Add regressions for final and intermediate symlink substitution, with victims in another owned temporary directory, asserting unchanged modes. Fix this before running the VM suite.

Evidence: `library/support-cleanup-repro.rs` and `library/support-cleanup-repro.log` in this attempt's output directory.

### B2 — BLOCKER, candidate-only: the decided alert suffix still fails to override a default

Reference: `finding:w0b1-recheck-r1`. Candidate location: `crates/tidemark-core/src/config.rs:1210`. Fix owner: **W0-B1 contract repair**. **Resolved in gate checkout `c8d7d84`.**

The exact candidate retains printable ASCII and uses `\xNN` for other suffix bytes, contrary to SCR-003's `/` and ASCII `[A-Za-z0-9._-]` allowlist with uppercase `%XX` for every other byte. A compiled probe with mount `/srv:blue` and disabled rule `fs_full_soon:/srv%3Ablue` leaves that disabled rule **and enabled** `fs_full_soon:/srv:blue` after two merges; validation reports no error.

Apply the allowlist/percent-encoding correction, update the rustdoc and conflicting expectations, and cover disabled overrides for colon, percent, space, backslash and Unicode. Those changes are already present in the gate checkout: the same probe leaves only the correctly named disabled rule, and `percent_encoded_default_names_and_disabled_overrides` passes. Evidence: `library/alert-suffix-{candidate,gate}.log`, probe source, and `contract-probe-results.json`.

### S1 — SHOULD-FIX: a required shared-type re-export is missing

Reference: `finding:w0-gate-component-state`. Location: `crates/tidemarkd/src/firstboot.rs:2`. Fix owner: **W0 contract-wiring repair, coordinated with W4-B's ownership**.

The file contains only its two ownership/purpose lines. SCR-002 requires `tidemarkd::firstboot::ComponentState`, re-exporting the sole definition at `tidemark-core/src/api.rs:72`. A consumer naming the documented daemon path fails compilation with E0425 in both revisions.

Add a documented `pub use tidemark_core::api::ComponentState;` to `firstboot.rs` and a consumer/type-identity compilation check. Do not introduce another struct. The other required re-exports are already correct: `tidemark_fs::throttle::GateStats` at `throttle.rs:6` and `tidemark_fs::lvm::VgInfo` at `lvm.rs:13`.

Evidence: `library/component-state-probe.rs`, both expected-negative compiler logs, and `contract-probe-results.json`.

### S2 — SHOULD-FIX: CLI dispatch erases typed command exit statuses

Reference: `finding:w0-gate-cli-exit-status`. Location: `crates/tidemark-cli/src/lib.rs:137`. Fix owner: **W0-B2**.

Each command returns `anyhow::Result<()>`; shared dispatch wraps every error as `CliError::Runtime`, whose exit code is always 1. Thus a later owner returning `CliError::Partial`, `Unavailable`, `Precondition`, `Unhealthy`, or a command-specific `Configuration` through anyhow cannot deliver the documented exit code from its owned command file. The existing `exit_codes` test constructs enum variants directly and bypasses this conversion. Current operational stubs correctly return 1, so this defect is latent until their implementations arrive.

Add a shared conversion that recovers a typed `CliError` from the anyhow error before falling back to Runtime, for example `error.downcast::<CliError>().unwrap_or_else(CliError::Runtime)`, and use it in dispatch. Test statuses 2–6 with and without anyhow context, plus a generic runtime error retaining status 1. A compiled seam probe using the actual crate types and exact mapping expression reproduced all five statuses becoming 1; the proposed conversion preserved them. This probe is not an end-to-end test of future commands.

Evidence: `library/dispatch-probe.rs`, `dispatch-{candidate,gate}.log` and `contract-probe-results.json`.

### S3 — SHOULD-FIX: W1-E cannot extend its error type within its declared ownership

Reference: `finding:w0-gate-store-error-owner`. Location: `crates/tidemark-store/src/lib.rs:28`; ownership: `docs/plan/waves/wave-1-primitives.md:13`. Fix owner: **W0 scaffold/wiring coordinator before W1-E dispatch**.

`StoreError` is defined in the shared crate root and contains only `NotYetImplemented`. Real storage I/O, SQLite and conversion failures need an implemented error representation, but W1-E owns only its six named modules and migrations. The shared-file exception permits dependency/module additions, not changing this enum. This prevents the promised isolated implementation even though all method signatures compile.

Move the error definition into a W1-E-owned module such as `writer.rs`, re-exporting it at the unchanged `tidemark_store::StoreError` path in a preparation change; alternatively explicitly assign the error region of `lib.rs` to W1-E. Preserve the public path and decide ownership before parallel work begins. Ordinary dependency/module additions are separately listed below and are already permitted by project policy.

### N1 — NIT: two public fixture fields have no rustdoc

Reference: `finding:w0-gate-fixture-rustdoc`. Location: `crates/test-support/src/random.rs:214`. Fix owner: **W0-C**.

`SizeDist::Uniform.min` and `.max` lack field documentation. The ordinary warning-free workspace documentation command passes because test-support does not enable `missing_docs`. A targeted `cargo rustdoc --offline -p test-support --features privileged --lib -- -D missing_docs` exits 101 on those two fields.

Document both inclusive byte bounds and enable `#![warn(missing_docs)]` in test-support's root so ordinary documentation checks enforce this part of the Definition of Done. Evidence: `library/support-missing-docs.log`.

### N2 — NIT, candidate-only: the claimed verbatim example fixture has drifted

Reference: `finding:w0b1-recheck-r2`. Candidate location: `crates/tidemark-core/tests/example.toml:11`. Fix owner: **W0-B1 contract repair**. **Resolved in gate checkout `c8d7d84`.**

The candidate fixture is 3,637 bytes versus the plan's 3,663: its throttle comment omits `or a user-defined profile`. Refresh it and compare the extracted section 3 TOML block byte-for-byte in the test. This is comment/test drift, not changed parsed configuration. The gate checkout contains the refresh and comparison at `config.rs:1248`; both are 3,663 bytes and its test passes. Evidence: `library/contract-probe-results.json` and the four-file tree comparison.

## Gate checklist

This table evaluates the supplied gate checkout. Candidate-only B2/N2 additionally fail its contract/acceptance checks.

| Wave 0 checklist item | Status | Evidence / remaining action |
|---|---|---|
| `cargo xtask ci` green and Rocky 8 container build green | **NEEDS-HUMAN** | `tools/ci.sh` passed locally: fmt, Clippy, 80 tests, owner/read-only scans. Container build has no completion evidence; W0-A explicitly marks it pending host. cargo-deny was skipped because unavailable. |
| Human review of core API against 03/04/05/07/08 | **NEEDS-HUMAN** | Prior review, decisions and recheck were read; R1/R2 are fixed in this checkout. No human API sign-off was established; S1 is outstanding downstream contract wiring. |
| Every stub file exists with correct owner | **PASS** | Audited all 86 explicit W0-A/W0-B owner-map paths: zero missing, misplaced or mistagged files. CI checks 120 crate source owner tags. File/header completeness does not establish the missing S1 re-export. |
| VM runner executes privileged smoke test on Rocky 8 | **NEEDS-HUMAN** | Runner and smoke tests exist; privileged compilation passed. No VM contacted. Repair B1 before execution; playbook requests both 8.8 and 8.10. |
| Tag `wave-0-done` and freeze core | **NEEDS-HUMAN** | Approval/release action after fixes and human checks. No tag, merge or freeze performed or claimed. |
| Privileged tests written, feature-only compiled, statuses listed | **PASS** | Three smoke tests and VG guard test listed below and in W0-C handoff. Modules/test target use `privileged`; no privileged `#[ignore]`; build passed. |
| Public rustdoc and commented unsafe confined to sys | **FAIL** | N1 is a concrete public-field documentation gap. No unsafe blocks were found anywhere in current sources; unsafe confinement otherwise passes. Standard rustdoc passed, targeted missing-docs check failed. |
| All task acceptance criteria evidenced in handoffs | **FAIL** | B1 breaks fixture confinement; S1/S2/S3 require preparation repairs. Rocky/container and privileged runtime acceptance remain pending. Existing local numerical/model/parser acceptance tests pass. |
| No unreferenced TODO or unexplained unwrap/expect | **PASS** | Source search found no TODO/FIXME/XXX in crates/xtask/tools. Literal `unwrap()`/`expect()` calls are in tests. Test-support infallible wrappers intentionally panic with context, as documented test-helper interfaces. |
| Handoffs written with deviations/gaps/testing | **PASS** | W0-A, B1, B2, B3 and C notes plus fix/recheck notes exist and were read. Historical pending-SCR claims require the decision precedence recorded below. |

No production mount, LVM mutation, service change or filesystem-metadata write was found. Existing non-test I/O is principally bounded config loading and CLI/daemon streams; scanner and persistence bodies remain stubs. Privileged fixture mutation lives in test-support, where B1 identifies a confinement violation. The source-policy scan is syntactic, not a proof against arbitrary aliasing.

## Wave 1 readiness

All **90 explicit public functions/methods** in W1-A–E and the referenced snapshot reader interface have matching stub signatures: A=10, B=2, C=5, D=21, E=52. Explicit struct field lists, `BulkstatIter` item/lifetime, and `FixedGate`/`AdaptiveGate` implementations of core `Gate` match. Name-only shapes deliberately remain for their owners. Error-free placeholders follow the supplied acceptance of SCR-005; none is being credited as runtime implementation.

The five tasks cannot currently proceed with **zero shared edits**. Dependency attachments are intentionally deferred and permitted; coordinate them once before dispatch or resolve their predictable manifest conflicts. S3 is a separate ownership gap that the ordinary shared-file exception does not resolve.

| Task | Interface/ownership readiness | Missing dependency attachments to implement the documented work |
|---|---|---|
| W1-A | All mount/blockdev/canonicalization/probe signatures exist; CLI probe file and dispatch exist. Host/root/recommendation fields remain owner design. | fs: `tidemark-sys`, `libc` for prescribed `makedev`, safe syscall support such as `rustix`. CLI: `tidemark-fs`, `tidemark-pg`. Dev dependencies: `test-support` for specified fixtures and `insta` where probe JSON snapshots are tested. fs `serde` if probe serialization is derived. |
| W1-B | Both signatures and all config/summary fields exist. `walk/mod.rs` already declares `worker` and `getdents`. | fs: `tidemark-sys`, `rustix`, `crossbeam-deque` for the prescribed work-stealing design; dev `test-support`. `globset` and `crossbeam-channel` are already attached. A custom benchmark harness may require a manifest entry. |
| W1-C | XFS signatures/fields/constant/iterator exist; generated bindings are future owned files. | sys: `libc`; dev `test-support` for loop-XFS fixtures. Bindgen is the prescribed one-time VM generation tool, not necessarily a Cargo dependency. |
| W1-D | ioprio, throttle, procfs and attribution signatures exist, with core `GateStats` re-export. | sys: `libc`. fs: `parking_lot`, `tracing`; safe proc-metadata support such as `rustix` where std is insufficient. Dev `test-support` where its privileged fixtures are used. The prescribed fs-to-sys ioprio caller belongs to W1-B. |
| W1-E | Reader/writer/catalog/time-series/retention/space signatures exist; resolve S3 before implementation. | store: `rusqlite`, production `serde_json` (currently dev-only), safe filesystem/statvfs support such as `rustix`; dev `tempfile` for isolated storage tests. `chrono` is an available implementation choice for ISO-week retention. |

The table distinguishes prescribed edges from conventional implementations: `rustix` is explicitly required by the walker, while safe host/proc/store operations may use it or other permitted wrappers; production `serde_json` and test `tempfile` are conventional choices for storage. Additional hot-path choices such as `rustc-hash`/`smallvec`, serde derives and date helpers depend on implementation; they are not missing frozen signatures. All third-party names above are already declared centrally. The internal workspace dependency entries `tidemark-sys`, `tidemark-fs`, `tidemark-pg` and `test-support` are **not** in `[workspace.dependencies]` and must be added there before member crates inherit them; only `tidemark-core` currently has an internal entry.

Shared-file inventory:

| File | W1 tasks needing/coordinating edits | Reason |
|---|---|---|
| Workspace `Cargo.toml` and `Cargo.lock` | A/B/C/D/E | Internal workspace entries and resolved member dependency edges. Centralize the preparation. |
| `crates/tidemark-fs/Cargo.toml` | A/B/D | Shared runtime/dev dependency attachments and any walker bench entry. |
| `crates/tidemark-sys/Cargo.toml` | C/D | libc and test dependencies. |
| `crates/tidemark-cli/Cargo.toml` | A | fs/pg probe calls and snapshot test dependency, with no other W1 owner overlap. |
| `crates/tidemark-store/Cargo.toml` | E | Storage/runtime/test dependencies. |
| `crates/tidemark-store/src/lib.rs` | E or preparation owner | S3 error relocation/re-export or explicit ownership assignment; optional migration module declaration. |
| `crates/tidemark-sys/src/lib.rs` | C, optional | Only needed if generated bindings are wired from the root; C can instead declare them inside owned `xfs.rs`. |

No required fs/CLI root-module or shared `mod.rs` wiring changes were found. B owns its existing `walk/mod.rs`; E can wire migrations from an owned module. Decide `test-support/privileged` feature forwarding with the dependency preparation: current member `privileged` features are empty, although workspace-wide feature builds succeed. Later per-package privileged tests need the helpers enabled.

Shared types have one definition: scanner records, byte paths, device stats, sample/report DTOs, `GateStats`, `VgInfo` and `ComponentState` come from core. Test-support's model and reference metrics are independent oracle representations converted to actual core `DirRecord`, not replacement scanner contracts. No additional cross-crate bytes/blocks or seconds/milliseconds drift was established. W0's fanotify probe, pg root re-exports, LVM interface and rate signatures are present. All 22 CLI commands and nested flag trees match 08; growth remains a CLI wrapper, writers uses positive `Duration`, hot keeps its documented fixed windows, and rendering reuses core helpers/`JsonDoc`.

## Open items and decision status

| Request | Status applicable to this review | Remaining work |
|---|---|---|
| SCR-001: snapshot widths, filters, bounds | **MODIFY / accepted with modifications** in 13 | Future owners implement checked SQL/ID/31-bit-slot conversions; no new decision sought. |
| SCR-002: API payload ownership and wire typing | **MODIFY / accepted with modifications** in 13 | S1 missing downstream re-export; future consumers retain wire/display rules. |
| SCR-003: effective defaults, profiles, alert rules | **MODIFY / accepted with modifications** in 13 | Candidate-only B2 remains; repaired in gate checkout. W5-D must consume core defaults and respect availability/thin-LV rules. |
| SCR-004: helpers, formats and scan IDs | **MODIFY / accepted with modifications** in 13 | W1-E must refuse existing scan IDs and callers retry; no silent replacement. |
| SCR-005: error-free inert stubs | **Accepted per supplied coordinator instruction** | No section recording that decision in 13 yet; handoff still says Pending. Supplied context assigns plan recording to a parallel task. |
| SCR-006: directory-inclusive apparent metric | **Accepted per supplied coordinator instruction** | 03/09 still state unconditional raw `du` equality; plan recording remains pending. Keep directory-inclusive `t_apparent`; normalize comparisons on GNU versions omitting directory sizes. |

All six SCR handoffs retain historical Pending lines. Document 13 explicitly supersedes 001–004; supplied task context supersedes 005/006. This review does not re-decide any SCR or claim the concurrent plan update has landed.

Complete remaining handoff inventory:

- **Pending host:** W0-A `:50/:79` Rocky 8/glibc 2.28 container build; exact Docker command is in that handoff. All tasks' cargo-deny/license/advisory audit awaits an equipped host; CI's permitted skip is not an audit result. Runtime dependency graph attachment remains W1 preparation work.
- **Pending VM, W0-C:** `loop_xfs_smoke`, `loop_lvm_thin_smoke`, `special_fixture_smoke`, and `lvm::tests::refuses_non_test_vg_without_commands` (`docs/handoff/W0-C.md:82`). The guard test itself issues no host commands, but was only compiled here. Actual loop/LVM setup failure rollback, cleanup failures and root special-node behavior remain unverified. The ordinary socket check was explicitly skipped with EPERM in this sandbox; FIFO/bytes/sparse/mode assertions ran.
- **Deferred VM work:** W0-A's helper/test gap is now implemented by C but runtime remains pending. B1 has no privileged core behavior. B2's scan/access/deploy checks belong to later command/daemon owners; B3's runtime parser/discovery/throttle/scan/store/rate/LVM checks belong to W1/W4/W5. No future tests against implemented runtime bodies exist yet because those bodies remain stubs.
- **Expected future-owner work:** B2 commands except completions and daemon normal startup are deliberate stubs; owners must implement canonicalization, runtime defaults, reports and calls. B3's name-only/private storage shapes remain owner work. W5-D reuses core default rules. Optional `FileHit.path` must remain honestly absent in query output; writer collision refusal and checked conversions remain storage/scan responsibilities. Install/fault script suites are later-wave work; the runner currently fails explicitly if a suite is missing. Find-oracle callers must account for mount-point/excluded entries, as documented.
- **Unassigned disclosed gap:** `docs/handoff/W0-B2.md:86` says shared table alignment counts Unicode scalars, not terminal-cell widths for East Asian/combining text. No follow-up owner was found. The renderer is frozen for later owners; assign an SCR-backed improvement if the gatekeeper requires terminal-cell alignment. This is a disclosed presentation limitation, not another proved frozen-contract violation.
- **Historical gaps closed in the gate checkout:** original core root DTO/owned `JsonDoc`/writer-window/growth-sort questions are settled in 13 and present in code; R1/R2 are fixed. Earlier handoffs' superseded limitations are not fresh open decisions. No unreferenced source TODOs were found.

## For the human gatekeeper

Repair B1 and the preparation issues above, then run these commands from the intended reviewed tree on disposable, provisioned Rocky VMs. `rocky88` and `rocky810` below are explicit assumed SSH aliases for **8.8** and **8.10**, not hosts contacted or verified by this review; substitute actual aliases if different.

```sh
cargo xtask vm-test --host rocky88 --remote-dir tidemark-wave0-review --suite cargo
cargo xtask vm-test --host rocky810 --remote-dir tidemark-wave0-review --suite cargo
```

Use the dedicated remote directory and prerequisites in `xtask/README.md`: XFS/LVM layout, spare image space, xfsprogs/devel, lvm2, bcc-tools, postgresql-server, lsof, SSH/rsync, compiler, pinned Rust 1.98.0, and a passwordless-sudo test user. The runner builds as that user and tests serially with explicit sudo PATH. Confirm all three smoke tests and the VG guard execute; verify test files are created, then mounts, loop devices, images and owned `tmtest_*` VGs are removed. Examine setup/cleanup failure behavior, which compilation cannot establish. Install/fault suites are not Wave 0 completion evidence yet. Run W0-A's recorded `rockylinux:8` container command and an actual cargo-deny audit on equipped hosts.

For core API approval, focus on the expensive-to-change seams: 03's non-additive disk versus charged/reclaimable metrics and measured directory contributions; raw-byte paths with separate display and UTF-8/base64 wire forms; scanner count/ID widths and Gate costs; snapshot SQL nullability, checked conversions, optional bulk filenames, collision refusal and filter ages; 07's exact live/sample fields and units; 08's method/flag/config surface, stable percent-escaped alert names, typed exit statuses and root-only permissions/access policy. Preserve one shared definition with the documented re-exports. Existing tests exercise whole envelopes and the golden fixture; they do not replace human approval of these contracts.

Human/coordinator actions still needed are API sign-off, actual platform/VM/advisory evidence, ownership/dependency preparation for W1, confirming the accepted SCR-005/006 plan record lands, deciding whether to assign the disclosed table-width limitation, and eventual tagging/freezing after repairs. No new group-access decision is needed: ADR-018 is root-only, with no `tidemark` group, socket_group key or UID gate added here.

## Build, tests and evidence

Both revisions used Rust 1.98.0 from `/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin`, `CARGO_HOME=/tmp/tidemark-w0-gate-cargo` copied from the host registry, and `CARGO_NET_OFFLINE=true`. Host cache contents were not changed and no network resolution was used. CI inherits offline mode because its subcommand accepts no extra arguments; direct Cargo commands used `--offline`.

| Check | Exact candidate | Gate checkout |
|---|---|---|
| `tools/ci.sh` | Exit 0; **79 passed, 1 scale test ignored**; 120 owner tags, forbidden-symbol scan, fmt and all-target Clippy pass | Exit 0; **80 passed, 1 scale test ignored**; same source/lint checks |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0 | Exit 0 |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | Exit 0 |
| cargo-deny within CI | Explicitly skipped, not installed | Explicitly skipped, not installed |
| Targeted core R1/R2 probes | Existing defects reproduced | Repairs reproduced; byte-equal fixture and disabled override |
| Missing re-export consumer compilation | Expected exit 1 / E0425 | Expected exit 1 / E0425 |
| CLI conversion probe | Typed statuses collapse to 1 | Typed statuses collapse to 1 |

Additional gate probes: cleanup reproduction compiled/ran with the external temporary mode changes in B1; targeted privileged test-support missing-docs check exited 101; a focused socket-visibility test passed its remaining assertions and printed the EPERM skip. GNU du 9.11 and GNU find 4.11.0-modified were present, so golden du/find oracle availability checks were not skipped. The standard suite includes `materialize_link_semantics_basic_matches_du_with_measured_directories`, `reference_reproduces_03_section_6_exactly`, deterministic random model/disk tests, 100-seed reference invariants, CLI surface tests, daemon config checks, stub contract tests and VM-command construction tests.

Artifacts are under this attempt's worker-output `library/`: `candidate-{ci,privileged-build,doc}.log`, `gate-{ci,privileged-build,doc}.log` and exit files; `contract-probe-results.json` and probe sources/logs; `support-cleanup-repro.*`; `support-missing-docs.log`; `oracle-environment.log`; supporting contract, readiness and support audit notes. Observation probes exit successfully when they reproduce the stated defect; the expected-negative compilation/doc checks are clearly distinguished from required builds.

Not checked: privileged runtime or VM connectivity, Rocky/container compatibility, actual cargo-deny advisories/licenses, the ignored 120,000-entry scale test, performance/soak behavior, fault-injected mount/LVM cleanup, future scanner/store/daemon integration, or human approval. No VM was contacted, no host mounts/LVM/services were changed, and no branch, merge, push, release tag or core freeze was performed. This report and its receipt are evidence candidates only.

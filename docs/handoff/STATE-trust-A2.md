# Handoff — STATE-trust-A2: storage integrity policy and complete test-target reporting

## Needs from coordinator

Run namespace/mount and real second-user acceptance on an unrestricted runner or disposable VM. Privileged tests are compiled only here, **pending VM**. Sandbox socket readiness failures and cargo-deny availability are recorded below. This is an evidence candidate, not independent verification or release approval.

## What changed

The existing no-follow descriptor opener now delegates ownership, mode, file type and link-count decisions to a pure function. Storage permits group/world read and execute permissions while refusing write permissions; creation remains 0700/0600. One process-wide atomic warning names the first readable accepted storage path. Both proc namespace files are read through a cached `OnceLock` once per process; read/parse errors fail closed. All public signatures in the part-A interface remain unchanged, including the retained `/proc/self/fd/<n>/.` anchor.

| Role | Refused when | Warned when |
|---|---|---|
| Ancestor | Not directory; owner neither root nor euid nor namespace overflow uid, unless read-only mount; group/world writable without sticky bit | Never |
| Sticky child | Not directory; owner neither root nor euid nor namespace overflow uid; group/world writable without sticky bit. Sticky writable children propagate the constraint to their children. No read-only exemption | Never |
| State directory | Not directory; owner differs from euid; group/world write bit | Group/world read bit, after acceptance |
| Used subdirectory | Same as state directory | Same as state directory |
| Leaf | Not regular file; owner differs from euid; group/world write bit; link count differs from one | Group/world read bit, after acceptance |

All path components still use no-follow opens or no-follow stat. Symlinks, FIFOs, hard links, foreign owners and unsafe writes remain refused without repairing metadata or modifying referents. Read-only mount ownership exceptions never excuse unsafe write modes. In the initial namespace overflow uid has no special treatment; ordinary effective-user ownership still applies. State directories, used storage subdirectories and leaves never receive the namespace-overflow exception. Sticky children in the ancestry receive root-like overflow ownership, while retaining the sticky-child write rule.

The pure internal signature is `fn policy(input: PolicyInput) -> Result<bool, &'static str>`, where `PolicyInput` contains `owner: u32`, `mode: u32`, `effective_uid: u32`, `user_namespace: bool`, `overflow_uid: u32`, `read_only_mount: bool`, `role: Role`, `links: u64`, `file_type: FileType`. `Ok(true)` requests a readability warning. The role enum is ancestor / sticky child / state directory / subdirectory / leaf. Table coverage includes every role, root/self/foreign/overflow ownership, initial and user namespaces, read-only ancestry, sticky chains, read vs execute vs write bits, wrong types and multiple links.

`cargo xtask ci` now invokes `cargo test --workspace --no-fail-fast`. It still stops after a failing Cargo test invocation, but Cargo reports all failing test targets within that invocation. No manifest, dependency, CLI source, daemon source, live source, plan or security document was changed. Shared-file edit: `xtask/src/main.rs` changes only this command argument.

The standalone fuzz catalog helper explicitly creates a 0700 temporary directory using the existing tempfile dependency. This is valid under the corrected policy and independent of umask. The standalone fuzz target cannot be built here; workspace parser smoke coverage remains available.

## Tests changed from refusal to acceptance

- `state_dir::tests::directory_policy_and_retained_fd`: an existing 0755 state directory is accepted; symlink refusals remain.
- `state_dir::tests::sticky_and_writable_ancestors`: a 0750 child of a sticky writable ancestor is accepted; non-sticky writable ancestry remains refused.
- New `state_dir::tests::readable_storage_warns_once`: in a fresh subprocess, a 0644 leaf is opened and rechecked, followed by 0755 state and 0750 used subdirectory acceptance. Exactly one captured warning names the leaf; its contents and mode remain unchanged.
- Existing hostile leaf, storage hierarchy, retained descriptor, CLI and scan lock regressions are retained. No prior leaf-readability refusal assertion was found in the owned integration tests.
- Daemon/deployment configured-directory 0750 refusal tests retain their stricter part-B rule.

## Manual fixture review

Checked the ownership, state path, and mode transitions in the workspace tests using unshare, uid changes or `set_permissions`; inventory retained in `library/fixture-inventory.txt`.

| Files checked | Finding |
|---|---|
| CLI `tests/rules.rs` | Namespace fixture's 0755 temp parent is euid-owned below sticky /tmp and now accepted; unmapped / and /tmp owners receive the ancestor-only exemption. State is created by mapped uid 0. Mount skip assertions untouched. DAC child owns its temporary parent and creates its state itself. |
| CLI `tests/e2e/support.rs`, `scan_reports.rs`, remaining e2e modules | Dropped-uid harness transfers temp/state ownership to child; 0755 accessible scan fixtures have no foreign writes. Mode-000 changes affect scanned objects, not state. State starts absent or is created privately with matching child owner. |
| CLI `tests/w4b/mod.rs`, `privileged.rs` | State is private or absent beneath private euid-owned temp ancestry; deliberate 0777/0750 state/deploy modes test the separate strict deployment preflight and remain refused. VM reboot state is supplied by deployment. |
| CLI `tests/w4a.rs`, `w6a3.rs` | Remoded executable/socket/locked fixtures do not weaken state ancestry. Command owner is private; state absent/private. Socket mode-000 permission tests remain separate. |
| Daemon `tests/w6a1.rs` | Configured state is private or absent; deliberate 0777/0750 refusal remains preflight policy. Recovery and symlink sentinels preserve checks. Readiness tests depend on forbidden sandbox sockets. |
| Daemon `tests/w6a2.rs`, `w6a2/runtime.rs`, `w6a2/privileged.rs`, `w6a3.rs`, `w6a3/operator.rs`, `w6a3/privileged.rs`, `w6a4.rs`, `w6a5.rs`, `w6b.rs` | State/temp/socket parents use private helpers and explicit 0700 where needed; missing state is created privately. 0640 target is a socket replacement sentinel; foreign 0666 staging file is a metrics refusal fixture, not valid storage. 0755 installed executable is unrelated to state. Snapshot cleanup children/files are private. Privileged service fixtures are build-only. |
| FS `tests/rules.rs`, `w1d/main.rs`, `w4a/lvm.rs`; live `w5a/privileged.rs`, `w5b/privileged.rs` | Foreign ownership, second-uid open-file and namespace fixtures exercise metadata/watch/procfs or executable reporting; they do not open Tidemark storage. |
| Store `tests/state_trust.rs`, `storage.rs`, module tests; scan `tests/state_trust.rs`, `w2b/main.rs`; test-support fixtures | Accepted state uses matching owner and no foreign writes; 0620, hard-link, FIFO, symlink and foreign-owner cases remain intentional refusals. Metadata fixture remodes are scan inputs. |
| xtask `tests/w6c.rs`, `w7c.rs` | Remoded 0700 shellcheck stubs are command fixtures, not state; no CI command list is pinned in w6c. |
| CLI deployment unit/scenario tests; daemon scheduler/API/firstboot unit fixtures | Deployment/private socket rules remain separate. Default 0755 storage and 0644 SQLite files now satisfy storage integrity; scheduler deploy children explicitly remain 0700. No out-of-scope source edits. |

## Corrected text for coordinator to apply

Plan: Before opening state locks, databases or snapshots, Tidemark walks directory ancestry without following symbolic links. Ancestors must be root/effective-user owned, or on a read-only mount; inside a non-identity user namespace, kernel overflow-uid ancestors are also accepted. Group/world writable ancestors must be sticky. Their children must be root/effective-user owned and non-writable by group/world, or sticky themselves with the same check applied to their children. State directories and used storage children require effective-user ownership and no group/world write bits. Leaves additionally must be regular single-link files. Existing readable storage is accepted with one warning per process naming the first readable path. Tidemark creates directories 0700 and files 0600. Deployment and daemon configured-state preflight retain their stricter privacy requirement.

Security: Custom storage paths require trusted ancestry and integrity checks. Symbolic links, special files, hard-linked leaves, foreign-owned storage and group/world-writable storage are refused without normalization. Readable/listable existing storage is accepted with a warning; creation is private. A sticky shared ancestor such as /tmp permits an owned non-writable child, including a 0755 child. Namespace overflow-owner acceptance is limited to ancestors in non-identity user namespaces, never state or leaves. Installed production access remains root only. These checks assume a trusted kernel/filesystem and exclude hostile root or same-uid processes. Descriptor anchors preserve previously established ancestry trust.

## Assumptions and limitations

The assigned coordinator decisions replace the earlier brief; no contract/spec changes were inferred from memory. No branch creation, merge, push, dependency addition, privileged runtime test or host configuration operation occurred. Proc map identity means exactly the three whitespace-separated values `0 0 4294967295`; namespace state is cached for this process, as requested. The two kernel proc files are trusted kernel metadata, read outside scan loops. Warning means once for the process, not a growing per-path cache. Unsupported proc reads fail closed rather than granting a namespace exemption. All long validation commands use timeout 1500 and the pinned offline 1.98.0 toolchain with four build jobs and a copied /tmp Cargo cache.

## Validation

Final-source checks below supersede the initial diagnostic runs. All long commands used timeout 1500; command/environment scripts and complete transcripts are in the attempt output `library/`.

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | Exit 0 | `final-fmt.log` |
| `rustfmt --check --edition 2021 fuzz/support/catalog.rs` | Exit 0 | `fuzz-format.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 | `final-clippy.log` |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | Exit 0 | `final-clippy-privileged.log` |
| `cargo test --offline -p tidemarkd --lib; explicit umask 022` | Exit 0; 79 passed, 0 failed, 0 ignored; 1 harness summaries | `final-daemon-022.log` |
| `Focused store state_dir tests` | Exit 0; 6 passed, 0 failed, 0 ignored; 1 harness summaries | `final-policy.log` |
| `cargo test --offline --workspace --no-fail-fast; explicit umask 022` | Exit 0; 800 passed, 0 failed, 14 ignored; 78 harness summaries | `final-workspace-022.log` |
| `Same full command; explicit umask 077` | Exit 0; 800 passed, 0 failed, 14 ignored; 78 harness summaries | `final-workspace-077.log` |
| `cargo build --offline --workspace --features privileged --tests; build only` | Exit 0 | `final-privileged-build.log` |
| `tools/ci.sh; explicit umask 022, offline Cargo` | Exit 0; 800 passed, 0 failed, 14 ignored; 78 harness summaries; cargo-deny unavailable, explicitly skipped | `final-ci.log` |

The initial full umask-022 and umask-077 passes also each reported 800 passed, 0 failed and 14 ignored. Final checks were repeated after extending root-like namespace overflow ownership to sticky children that are themselves ancestors. Final Cargo CI source checks passed with 147 owner tags. No dependency was added or fetched.

Totals sum harness summaries, including subprocess/helper summaries and conditional early returns; they do not count namespace or listener scenarios as exercised. Focused `--nocapture` checks confirm `binary_ready_and_stopping` reports “Unix sockets unavailable in sandbox; readiness test deferred to CI”, and `namespace_mounts_are_skipped` reports `unshare: write failed /proc/self/uid_map: Operation not permitted`. Both return early (`socket-limit.log`, `namespace-limit.log`). Contrary to the task's anticipated five readiness failures, this baseline's existing socket guards produced no failing targets. No guards or mount assertions were changed. The root/foreign-owner store regression and existing bind-mount/namespace privileged tests remain **pending VM**. Advisory/license validation needs an equipped cargo-deny runner. Standalone fuzz compilation remains unavailable here; its helper was independently formatted and the workspace parser smoke test passed.

The final pure table includes namespace overflow sticky-child acceptance, initial-namespace overflow sticky-child refusal, and foreign sticky-child refusal even on a read-only mount. This applies the coordinator's root-like overflow ownership to ancestry while preserving effective-user-only ownership for actual storage.

No canonical memory delivery was pending in receipts at the initial or post-test checkpoints. No memory projection was edited. Implementation, tests, the CI argument, fuzz helper and handoff are confined to the four owned changed files; no public signatures changed, no unsafe code was added, and no out-of-scope daemon fixture was edited. Independent review and runtime guest acceptance remain coordinator work.

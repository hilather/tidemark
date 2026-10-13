Reviewed commit: `1c8a0be4a30e2eee9ed498623a7d658b5b562ca7` (`git rev-parse HEAD` at review start).
Verdict: **CHANGES REQUIRED**.
Counts: **1 BLOCKER / 2 SHOULD-FIX / 0 NIT**.

## Second-audit F01–F06 dispositions

These identifiers refer to the findings in `docs/reviews/W7-C1-audit2.md`, not its older-audit disposition table. “Resolved” describes the inspected fix, not VM acceptance.

| Finding | Disposition | Source evidence and regression that discriminates the old defect |
|---|---|---|
| F01 — state trust | **PARTLY RESOLVED** | Ordinary initial-namespace ownership, no-follow, single-link and no-foreign-write checks are now shared by lock/catalog/series/snapshots and daemon/deployment. Unsafe ancestor-owner exemptions and mutable purge descendants remain: R3-01/R3-02 below. `state_dir.rs:53,171,216,314,327`; store `tests/state_trust.rs:37,75,131`; scan `tests/state_trust.rs`; CLI `tests/state_trust.rs`; deployment `marker_never_waives_owner_or_write_permissions`. These ordinary temporary-fixture assertions can execute at uid 0. Real foreign-owner tests are privileged/build-only; simulated foreign metadata is not a second-user exercise. |
| F02 — metrics parent | **RESOLVED** | `tidemarkd/src/metrics.rs:598` calls `trusted_directory` on every pass; its descriptor survives staging, rename and sync at `:645–646`. Refusal degrades/retries at `:706–731`. `w6b.rs:724` (`metrics_refuses_writable_ancestry_without_creating_or_changing_files`) and `:743` (`exporter_rechecks_directory_each_pass_and_recovers_after_symlink_replacement`) have real mode/unchanged-target assertions without socket or uid skips. The ancestry exceptions in R3-01 remain a shared-policy concern; the original ordinary writable-parent/symlink case is repaired. |
| F03 — procfs pathname bytes | **RESOLVED** for the original multi-GiB amplification | `tidemark-fs/src/procfs.rs:44–77` accounts record-vector and target capacities, including old/new record storage; `tidemark-live/src/procscan.rs:188–305` preflights inventory and downstream row/scratch budgets before history mutation. `fs/tests/w1d/budget.rs:20,60,74`, `live/tests/w5b/unit.rs:321,358,387`, and daemon `src/lib.rs:216` assert rejection/preservation and typed OOM handling without privilege skips. The exact scope of the memory bound is qualified below. |
| F04 — symlink atime/boundary | **RESOLVED under the explicitly documented atime exception** | In-mount link atime can still change; this is disclosed in CLI `cli.rs:69` and operator guide `:5,384`. Target traversal now checks mount/identity and pins links before readlink: `symlinks.rs:259–320,344,356–370`. `fs/tests/w3b.rs:334` exercises absolute, relative and intermediate escapes through the actual resolver with a substituted mount boundary. The cross-mount atime test at `:372` is privileged/build-only. This is not a claim that link atime is unchanged. |
| F05 — optional advisory gate | **RESOLVED in the workflow** | `.github/workflows/ci.yml:18–57` has no job-level allowance; the final aggregator fails on any failed advisory/license/ban/source step. `xtask/README.md:3–18` labels the reduced local check and documents `--require-deny`; `xtask/src/main.rs:42–46` makes missing strict tooling fatal. Existing xtask pure tests cover deny policy, and an executed review-only structural test verifies all four workflow outcomes. Actual advisory execution and repository branch protection are unverified. |
| F06 — terminal diagnostics | **RESOLVED** for the reported boundary | Deployment path errors escape bytes at `deploy/files.rs:437–439`; final human-output boundaries escape diagnostic strings in `cmd/deploy/render.rs:13–18` and `lib.rs:177–210`. Pure tests at render `:91`, CLI lib `:229`, and deploy tests `:1253` discriminate ESC/newline/invalid-byte regressions without privileges or sockets. Machine JSON remains separately encoded. |

## Findings, most serious first

### R3-01 — BLOCKER — Two ancestor exceptions do not establish the immutability required by SQLite pathname reopens

Reference: `finding:w7-review3-security-r3-01`.

**Sites:** `crates/tidemark-store/src/state_dir.rs:78–83,99–110,152–157`; `crates/tidemark-store/src/timeseries.rs:37–69`; `crates/tidemark-store/src/reader.rs:298–300`. The policy accepts any overflow-uid ancestor in a nonidentity user namespace, and any foreign-owned ancestor on a read-only mount. SQLite subsequently reopens the original pathname, not the checked directory descriptor. Both exceptions can admit a parent that an ordinary host user can rename through another view. A marker and final euid ownership do not remove that parent's rename authority. The kernel maps unmapped stat owners to overflow uid and checks DAC against initial-namespace identities; see [user namespace semantics](https://man7.org/linux/man-pages/man7/user_namespaces.7.html).

**Concrete scenarios, inferred from source; not executed end to end here:**

1. An administrator runs the direct CLI as namespace uid 0 with mapping `0 0 1`, using a root-owned 0700 state directory below an ordinary host user's 0755 directory. An unmapped host owner becomes overflow uid, so the parent passes the exception. That host user can rename the state entry in their own parent and replace it with a link after the last checked-leaf call, before `Connection::open_with_flags(path, flags)`. A link to another host-root-owned directory containing an empty/compatible `timeseries.db` redirects migration/writes. Namespace uid 0 mapped to host uid 0 has ordinary owner access to those files; this scenario does not depend on retaining initial-namespace DAC capabilities. Earlier scan-lock inode checks do not cover a later SQLite reopen. Initial-namespace root rejects this foreign parent and is not vulnerable through the overflow exception.
2. In the initial namespace, the administrator has a read-only bind view of a tree whose backing parent is still writable by its ordinary owner through another mount. Root reads a catalog or snapshot through that view. `fstatvfs(RDONLY)` does not prevent that owner renaming the backing state entry through the writable alias. Replacement between validation and SQLite reopen can redirect a read to an outside database or a FIFO. A review-only Python SQLite read-only FIFO probe timed out, supporting the blocking sink; this is not a candidate-rusqlite race reproduction. A writable direct scan through an entirely read-only state mount may fail earlier; that is not the claimed scenario.

Correct default state beneath trusted root-owned parents is protected. The already-retained daemon descriptor does not follow a later ancestor replacement; the vulnerable sinks are callers reopening an ordinary lexical pathname after these exemptions admitted mutable ancestry.

**Recommended change and exact minimal file set:** revise the ancestor policy in `crates/tidemark-store/src/state_dir.rs` to fail closed for unmapped/overflow owners and foreign owners on read-only mounts, unless a stronger provenance mechanism proves their authority. Update its policy-table expectations and add real unmapped-owner and read-only-bind/writable-alias regressions in `crates/tidemark-store/tests/state_trust.rs`. This challenges the supplied overflow-owner coordinator decision and requires that decision to be revisited; it is not permission to silently change a frozen contract. A broader alternative is to retain verified directory handles through every SQLite connection/publication and use only those anchors, including backups and sidecars; merely adding SQLite NOFOLLOW to the final leaf does not protect ancestors or companions. The report recommends the smaller policy repair, not an unreviewed broad interface change.

**Required regression:** use a disposable guest's second uid and a deterministic checkpoint immediately before SQLite reopen. Require refusal before any outside file read/write or lock/publication; preserve sentinel bytes/inodes/modes. Separately retain positive root/euid-owned sticky ancestry and already-verified proc-descriptor cases. Existing pure tests currently encode acceptance of these exceptions and cannot establish hostile-host safety.

### R3-02 — SHOULD-FIX — Purge descends into mutable state descendants before checking their provenance

Reference: `finding:w7-review3-security-r3-02`.

**Sites:** `crates/tidemark-cli/src/deploy/purge.rs:90,115–135,142–149`; immediate-child inventory in `crates/tidemark-cli/src/deploy/files.rs:485–498`.

Purge verifies its root, marker and parent, but both preflight and deletion walks open every descendant directory without requiring euid ownership or excluding group/world writes. The final directory-entry identity check runs only after contents have been unlinked. Unknown children and deeper snapshot descendants bypass the immediate-name inventory.

**Scenario:** state was preseeded with a writable/foreign-owned child before its root became private, and an ordinary user retained a descriptor to that child. It contains a root-owned private descendant. During an authorized root purge, that user can move the descendant through the retained writable parent into an outside directory after purge opens it. Purge then deletes through its retained descriptor outside state and only afterward notices the name disappeared/changed. This requires preseeded unsafe descendants; it does not grant an ordinary user access to an already-correct private state tree. The user need not be able to read or unlink the root-private descendant's contents themselves.

**Executed evidence:** a small program linked against this checkout's `tidemark_cli` calls production `unstage(..., true, fake_systemctl)` with a private valid root/marker/config and an unknown 0777 child. Admission reaches the fake service stop and production purge succeeds; no real service command runs. A second temporary-fixture program copies the current walker with one deterministic after-open rename hook: it returns ENOENT after deleting the sentinel from the moved directory outside the fixture's state root. The latter verifies the ordering defect in an instrumented copy, not a two-uid production race. Both run only in disposable `/tmp` fixtures; source and logs are in the attempt artifacts.

**Recommended change/files:** `crates/tidemark-cli/src/deploy/purge.rs` must validate the opened descriptor of every directory before descent in both preflight and deletion, requiring euid ownership and no group/world write permissions; preserve mount/depth/no-follow checks. `crates/tidemark-cli/src/deploy/tests.rs` needs unknown-child and nested writable/foreign-owner refusal tests, plus a deterministic retained-fd rename regression that refuses before the service call or any unlink. Existing leaf symlinks can still be unlinked as entries without following them; the missing protection is mutable directory ancestry. Real two-uid validation remains pending VM.

### R3-03 — SHOULD-FIX — Watcher count caps still permit pathname retention above the daemon memory limit

Reference: `finding:w7-review3-security-r3-03`.

**Sites:** `crates/tidemark-live/src/watch/mod.rs:948–983`; `watch/rings.rs:104–118,198–237,364–438`; defaults in `tidemark-core/src/config.rs:248–252`; `packaging/systemd/tidemarkd.service:22`.

Each watcher retains up to 200,000 full dirty parent paths without a byte budget or timed expiry. Ring entries separately own directory keys as map key, HotEntry key and (when attribution has a unit) units-map key; seen identities own another key copy. Completed minute maps remain after persistence draining. These count caps do not constrain their bytes to the service's 2 GiB limit. This consumer predates filesystem mode, but is in the requested tree scope and receives the new mode's wider event population.

**Concrete conditional scenario:** an ordinary user can create/write a sufficiently large tree on each default watched filesystem (`/` and `/data`), their writer has normal systemd-unit attribution, and no successful scan or transport loss clears dirty state during the interval. Feed 200,000 distinct approximately 3,900-byte parent paths per watcher at a sustainable rate; retain 1,000 distinct directory keys in each of 59 completed minutes, then send 70,000 distinct-key file closes per watcher during the current minute. First-six-component hot keys can exceed 1,000 bytes under `/home/<user>` and 1,250 under `/data/<user>` using ordinary 255-byte names. All paths can remain below 4096 bytes; only a few attacker fds need be open simultaneously. Sufficient inode/disk quota and sustainable event throughput are explicit prerequisites.

| Retained payload alone, across both watchers | Bytes |
|---|---:|
| Dirty parents: `400000 × 3900` | 1,560,000,000 |
| Completed keys: `59000 × 3 × (1000 + 1250)` | 398,250,000 |
| Current rows after the count-cap halvings: `20000 × 3 × 2250` | 135,000,000 |
| At least 66,000 directory identities before the shared 200,000 seen cap: `66000 × 2250` | 148,500,000 |
| **Total** | **2,241,750,000** |

That exceeds 2,147,483,648 bytes without counting allocation/tree/hash overhead, unit/command strings, inode-size history, daemon minute-history copies, SQLite, procscan or any scan. `persisted()` marks a bucket drained but keeps its maps, so normal service draining does not invalidate this count. This is a source-derived lower bound, not an executed OOM/load test. An actual kernel queue overflow clears dirty state; the scenario requires pacing below that threshold. Rich FID mode has additional full-path cache copies, but the calculation above applies to Rocky fd mode.

**Recommended change and files:** introduce a documented aggregate byte/capacity budget across dirty paths, ring keys/history, FID caches and returned/queued copies. Coalesce excess dirty paths to the watched root and mark loss while preserving event totals; make memory admission/reservations fallible. Include `take_dirty`'s clone-before-remove and ranking/output temporaries. Implementation/review set: `crates/tidemark-live/src/watch/mod.rs`, `fidmode.rs`, `rings.rs`, `crates/tidemarkd/src/services/watcher.rs` (shared watcher/history budget), and `crates/tidemark-live/tests/w5a.rs` plus unit modules in those files. No new config key is necessary if a documented internal cap suffices; any contract change needs normal review. A small-budget synthetic regression must assert retained capacities and temporary copies under long names, repeated minute rollover and multiple watchers, as well as deterministic root fallback/loss and continued totals. Existing count-only tests would not fail on this defect.

## F01 matrix: audit2 “Checks performed today”, row by row

| Audit2 boundary | Current evidence and remaining qualification |
|---|---|
| CLI state selection | Filesystem trust is enforced when direct preflight enters `ScanLock::acquire` (`orchestrator.rs:709`), before lock/database creation. Absolute-path config validation alone is no longer relied on. Actual CLI hostile-database test checks unchanged sentinel and no extra state entries. R3-01 exceptions apply. |
| Direct scan admission/lock | State and the full database companion family are checked before `open_leaf(scan.lock)`; no-follow/nonblocking open and fstat type/owner/mode/link checks precede flock (`orchestrator.rs:709–725`, `state_dir.rs:327–336`). Scan state-trust matrix covers links/FIFOs/hard links/unsafe writes. |
| Direct/catalog database | `catalog.rs:119–121`; `timeseries.rs:37–63,115–137` validate parent/main/WAL/SHM/journal/backup family and read the header through a checked nonblocking fd. The final SQLite reopen is still lexical at `:69`; R3-01 means its ancestry proof is incomplete. |
| Snapshot writer | `writer.rs:32–79` verifies the hierarchy, final/temp names and exclusive no-follow/nonblocking temporary creation. Later publication/drop remain pathname operations protected by the assumed ancestry. `storage::writer_refuses_hostile_snapshot_leaves_and_directories_without_changes` is the regression. R3-01 remains relevant. |
| Snapshot reader | `reader.rs:293–300` checks hierarchy and opened leaf before immutable read-only SQLite. Ordinary symlink/FIFO/hard-link/unsafe-mode cases are covered by store `snapshot_reads_refuse_hostile_leaves_and_hierarchy`; pathname reopen still depends on R3-01. |
| Corrupt catalog recovery | `catalog.rs:382–431` checks scan-lock identity/flock; `:434–450` validates database family and snapshots before creating the recovery namespace. Preservation/moves remain under the verified path assumption (`:452–487`). Busy/wrong-lock regression preserves files. |
| Crash temporary cleanup | `catalog.rs:545–597` starts at StateDir, validates used snapshot children, retains mount/entry descriptors and unlinks checked regular temporaries. Width is streamed. Unsafe leaf/hierarchy store regressions and existing crash recovery tests cover it; same-device bind execution remains VM-only. |
| Scan retention | `orchestrator.rs:1034–1053` requires exactly `snapshots/<root>/<name>.db`, opens each checked directory and unlinks through its descriptor. The old canonicalize-to-unlink pathname race is removed. |
| Daemon state preflight | Shared checks at deploy `files.rs:322–418,442–585`; daemon `lib.rs:1447–1497` retains admitted handles. Markers never waive owner or write checks; only trusted-owner state read/execute mode can be normalized. Deploy remains private. Injected foreign-owner assertions are not real chown/second-user evidence. |
| Daemon persistent files | The complete main/WAL/SHM/journal/backup/lock/marker/record inventory uses checked leaves; used snapshots/tmp/deploy children are checked. Daemon `tests/w6a7.rs` asserts actual binary refusal before listener/service initialization for hostile leaf families. Snapshot descendants are checked when storage uses them; purge is the exception in R3-02. |
| Deployment source/config reads | Component traversal uses shared trusted ancestry, final nonblocking/no-follow bounded regular-file read, and captured bytes drive parsing/hashing. This intentional operator-selected source is not itself a root-owned-state leaf. R3-01 affects shared ancestor assumptions. |
| Stage state operations | `deploy/mod.rs:329–405,418–440` retains installed-config parent through publication/compensation; `config_publication_retains_verified_parent_after_path_replacement` tests the replacement checkpoint. Unsafe state leaves are checked before deploy-child creation and service calls. |
| Ordinary unstage | `deploy/mod.rs:686–708` validates state/marker/config before stopping and descriptor-relative record removal. It does not recursively delete without the explicit purge option. |
| Purge | Root/marker/parent repairs are at `purge.rs:44–90`; bounded same-mount traversal and post-walk inode checks remain. Descendant provenance is incomplete: R3-02. |

### Audit2 repair list, point by point

1. **Ancestry walk/retention:** implemented at `state_dir.rs:171–191,238–247`, with component DIRECTORY/NOFOLLOW, NOATIME and EPERM fallback and post-create fstat. Sticky chains validate each child. Namespace overflow and read-only owner exceptions fail the required “untrusted users cannot replace ancestry” proof (R3-01).
2. **State/subdirectory/marker trust:** state and used children require euid/no foreign write; newly created paths are checked. Supplied integrity-policy decision intentionally permits read bits with a once-per-process warning (`:140–145`); daemon/deploy configured-state privacy is stricter. Marker ownership/write checks are never bypassed. Recursive purge needs the same child policy (R3-02).
3. **Leaves/SQLite/recovery/deletion:** main and all specified companions, locks and snapshots have regular/euid/no-foreign-write/single-link checks before reads or mutation. SQLite NOFOLLOW is not used on proc-fd paths because the bundled SQLite rejects their magic-link ancestor; a regression checks that behavior. Trusted ordinary ancestry makes a lexical reopen safe against other uids, but R3-01 prevents claiming that for every accepted path. Retention is descriptor-relative; writer/recovery pathname steps still rely on ancestry.
4. **Adversarial regressions:** existing tests cover symlink/FIFO/hard-link/unsafe-mode families, marker ownership simulations, unsafe known descendants, retained daemon/config handles and unchanged sentinels. A new review-only probe actually reopened a second StateDir from the first descriptor, replaced the original name with a symlink, and confirmed Catalog wrote only into the retained original; making that referenced directory writable was rejected. Real unmapped owners, bind aliases, recursive-purge mutable descendants and two-uid races remain missing acceptance cases.

## Scope, review method and sandbox limits

The source review uses the checkout above and `git diff fb8021c..HEAD --stat` (193 files). Read the project instructions, prior audit, required fix notes, W7-C guidance and relevant source/tests; notes supplied leads rather than proof. No plan drift was scored. Only this review file changes the repository. No source fix, branch creation, merge, push, VM access or host service/mount/LVM operation occurred.

The exact blind architecture candidate is `5d3a437e2b509ff679cbb54f783b8ab25db595ef`, base `6a277e7f7d90266d139393d96c27695628f93027`, scope **candidate_tree**. Its delta changes only `tidemarkd/tests/w6a1.rs` and its handoff: bounded test rendezvous/process isolation; no defect identified in that delta. Its production source is identical to the reviewed checkout (only CLI test `w6a3.rs` and documentation differ), so the whole-tree production findings above also apply to the exact candidate. The telemetry receipt is separate from the report-only deliverable submission; neither is an acceptance decision.

Sandbox evidence: `id` reports uid 0; `umask` is 022; `/proc/self/uid_map` is `0 0 1`; `setpriv --reuid 65534 --regid 65534 --clear-groups id` fails with `setresuid: Invalid argument`. Consequently no ordinary-second-user execution is claimed. Mode/link/FIFO/no-follow tests still make real assertions here; root/foreign-owner branches requiring an actual second uid and all privileged-feature tests remain build-only. Unix-socket and namespace tests can return early and are not listener/mount evidence.

## Watcher, symlink, proc inventory and other boundary review

All 14 unsafe sites in `tidemark-sys/src/fanotify.rs` were inspected; every site has a SAFETY comment. No memory-safety defect or descriptor leak was found in the inspected first-party paths. The exact inventory follows; this is source evidence, not a target-kernel or Miri result.

| Lines | Operation and safety argument |
|---|---|
| 88, 99 | Integer-only group initialization; adopt a nonnegative unique fd once. Group is NONBLOCK/CLOEXEC; event fds are RDONLY/LARGEFILE/CLOEXEC. |
| 113 | Read only the notification group into the supplied live mutable slice/length; successful kernel count selects its initialized prefix. No event-file payload read. |
| 129 | Mark call borrows a live group and NUL-terminated CString. The caller uses its retained mount descriptor's proc path. |
| 193 | Initialized `[u64;17]` provides 136 aligned bytes: eight-byte header plus 128-byte tail. Capacity, empty C string and mount-id output live through synchronous `name_to_handle_at(AT_EMPTY_PATH)`. |
| 207 | Read the initialized header; reject oversized result and invalid mount id. Failed identification leaves `identified=false`. |
| 221, 226 | Reopen uses the initialized bounded handle and borrowed retained mount; first O_PATH/CLOEXEC, then only on EINVAL RDONLY/NONBLOCK/NOFOLLOW/NOATIME/CLOEXEC. No branch reads payload or drops NOATIME/NONBLOCK. |
| 242 | Adopt exactly one successful new reopen fd; errors own no returned fd. |
| 259 | Copy two four-byte header fields and at most 128 handle bytes into an initialized aligned 136-byte local buffer; input cannot alias the fresh local storage. |
| 265, 276 | Existing FID helper opens with O_PATH/CLOEXEC and adopts only a successful unique fd. |
| 397 | Adopt a kernel-origin event fd once before semantic decode; errors drop it. EventIter construction is private; public fuzz decoding has no descriptor ownership. |
| 673 | Test-only F_GETFD checks EBADF without adopting the integer; its subprocess avoids concurrent descriptor-number reuse. |

`open_by_handle_at` interprets the handle relative to the watched mount's filesystem. With O_PATH a symlink handle is not dereferenced; a non-O_PATH symlink handle fails ELOOP. Invalid/deleted handles return ESTALE. These semantics were cross-checked against the [Linux syscall documentation](https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html); no forged-handle target-kernel experiment was run. Production handles come from the kernel event descriptor/group, not user-supplied API bytes. Fd/filesystem identification/reopen failure retains original-fd metadata and counts the event at the watched root; rich-mode resolution failure marks loss and clears caches (`watch/mod.rs:799–805,851–905`). No indefinite ESTALE retry was found.

O_PATH avoids FIFO peer waits and device open operations. The read-only fallback retains NONBLOCK and NOATIME, never reads the result, and rejects a symlink handle; NONBLOCK alone cannot prove every device driver's open is side-effect-free. Upstream [Linux v4.18 fanotify source](https://raw.githubusercontent.com/torvalds/linux/v4.18/fs/notify/fanotify/fanotify.c) filters path events to regular files/directories, supporting the production assumption. Rocky backport source and the actual EINVAL fallback were not executed, so a universal device-driver guarantee is not claimed. A type gate before the data-capable fallback would make that assumption explicit. Metadata syscalls can still wait for filesystem I/O; there is no per-syscall wall-clock timeout.

Unread iterator records are drained on drop (`fanotify.rs:426–430`); collected events own their fds and stop/error/own-pid paths drop them. Fd batches are capped at 256 events/6144 bytes; rich reads at 65536 bytes. A foreign-mount event adds one scoped reopened fd. Group/mount/shutdown fds are owned and worker shutdown joins. The new 136-byte handle storage does not allocate per event, but readlink, RawPath conversion, parent/key construction and aggregation do allocate; this is not an allocation-free event path. R3-03 is the remaining aggregate memory defect.

Filesystem mapping checks originating mount id and resolves foreign events through the retained watched mount; metadata still comes from the original descriptor (`fdmode.rs:110–156`). Outside/unresolved paths use watched-root attribution with a saturating unmapped counter. Hard links can select another alias, and a watched subdirectory bind can receive writes elsewhere in its filesystem that count at root; precise directory attribution/exclusion is therefore best effort. Control bytes remain byte paths until escaped rendering. Existing pure mapping, fallback, batch/descriptor-drop and mark-selection tests contain substantive assertions; namespace/bind/chroot and unread-close tracing remain VM-only.

Symlink target traversal checks known nested mount boundaries before metadata (`symlinks.rs:270–273`), uses SYMLINK_NOFOLLOW/NO_AUTOMOUNT statx (`:278–289`), pins O_PATH/NOFOLLOW then rechecks type/inode/mount (`:299–310`), reads the pinned link (`:320`) and opens directories through that descriptor (`:344`). Absolute targets and `..` outside the selected mount stop without following them. The stack is bounded (4096), expansions are bounded (40), and pending/path bytes are bounded (1 MiB). On old kernels lacking mount IDs, a same-device bind newly inserted after mountinfo can evade the static boundary list; that requires mount-management authority visible to the process, not merely an ordinary user's symlink rename. It remains a VM limitation, not an executed bypass here.

The proc inventory's 128 MiB admission accounts vector capacity and target capacity and bounds the temporary target. Downstream preflight has 4 MiB row-set and 16 MiB scratch limits, exact admitted reservations and 10,000 histories of 60 samples (`procscan.rs:318–340,471–482`), with typed resource-pressure degradation in daemon `lib.rs:693–704` and service retry at `services/procscan.rs:133–135`. `Inventory::bytes()` is not literally a peak-allocation meter: the fixed 8 MiB allowance covers the two retained numeric-ID vectors but not simultaneous old/new fd-ID allocations in `ids()` (`procfs.rs:120–123`), up to about 4 MiB extra. The separate 6 MiB transient allowance can cover that nonoverlapping phase; no violated 512 MiB envelope was established. The envelope test checks constants, not measured RSS, allocator overhead or every concurrent daemon component.

Offline snapshot mutation rechecks definitive daemon absence before and under ScanLock (`cmd/snapshots.rs:70–91`, `cmd/scan.rs:316–338`); indeterminate transports refuse. Daemon corrupt/missing catalog recovery waits for scan ownership (`lib.rs:1173–1212`), and checked recovery verifies the supplied lock itself. A version-0 catalog's read-only Invalid result is not classified as missing/corrupt: busy-lock startup can still migrate it via ordinary writable open. This is a lifecycle limitation needing an explicit migration-lock regression, not an established unprivileged exploit after the trust repair.

API framing remains bounded to 1 MiB including newline (`api.rs:55–86`), after root peer authorization (`:397–403`), with deadline/shutdown checks and 32 retained handler permits (`:431–486`). Hidden fanotify fuzz decoding returns a nonowning integer and never adopts/closes fabricated descriptors; production ownership stays private. Mount-id parsing is pure and production reads retain a 16 KiB cap. No harmful network/API reachability expansion was found from the hidden seams. Parser tests are real pure assertions; actual listener credential/deadline integration is sandbox-limited.

## Validation performed

Commands used pinned Rust 1.98.0, an attempt-local copy of the readable Cargo cache, `CARGO_NET_OFFLINE=true`, four build jobs, two test threads and explicit umask 022. Each long Cargo command ran under `timeout 1500`. No dependency was fetched. Artifact names below are relative to this attempt's worker-output `library/`.

| Check | Observed result | What it establishes |
|---|---|---|
| `timeout 1500 tools/ci.sh` | Exit 0; `ci.log`; 845 passed, 0 failed, 14 ignored across 79 printed harness summaries | Source policy, format, Clippy with warnings denied, workspace tests and privileged compilation completed. Counts include subprocess/helper summaries and conditional returns, not 845 unique independently exercised scenarios. |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | Exit 0; `privileged-build.log` | All privileged test targets compile; none were run. |
| Exact `w6a1::binary_ready_and_stopping`, `--nocapture` | Exit 0 with explicit Unix-socket-unavailable early return; `socket-limit.log` | **Did not assert readiness/listener behavior here.** |
| Exact CLI rules `namespace_mounts_are_skipped`, `--nocapture` | Exit 0 after uid-map EPERM; `namespace-limit.log` | **Did not assert namespace/mount behavior here.** |
| Actual-library retained-directory probe | Exit 0; `retained-probe.log`, source `retained_probe.rs` | A second proc-form StateDir remains bound across rename/link replacement, and final write-bit policy still applies. One uid, no actual second-user race. |
| Production purge admission probe | Exit 0; `purge_admission.log`/`.rs` | Real unstage/purge accepts an unknown writable descendant; service controller is a fake, all deletion inside its disposable temporary fixture. |
| Instrumented purge checkpoint | Exit 0 with assertions; `purge_checkpoint.log`/`.rs` | Copied walker deleted the moved outside-state sentinel before its final identity check; no production two-uid race claim. |
| Dependency-workflow structure | PASS; `deny-workflow-structure.log`/`.py` | No job-level allowance and fatal aggregation of all four deny step outcomes. |
| Supporting Python SQLite FIFO read-only probe | Expected two-second timeout, child killed/reaped; `sqlite-readonly-fifo-probe.log`/`.py` | Supporting blocking-sink evidence, not the candidate's bundled SQLite race. |
| `timeout 1500 cargo clean` | Exit 0; `clean.log`; removed 30,180 files / 19.4 GiB | Build artifacts cleaned before submission. |

The CI log explicitly reports **cargo-deny unavailable and skipped**. Its exit 0 is not an advisory/license check or release-gate approval. The required strict workflow behavior was inspected and structurally checked, not executed against advisory data.

For the regressions relied on in F01–F06 and watcher review, the CI log records success for the actual hostile database/lock/reader/writer matrices, retained-fd tests, writable ancestry/metrics restoration, selected symlink boundary, byte-budget admission/history preservation, pure diagnostic escaping, recovery lock checks, API/fuzz parsers, descriptor-drop subprocess, mapping/mark fallback and bounded synthetic fd-batch tests. Inspection found no uid/socket early return in those assertions. Deployment/daemon foreign-owner *simulations* ran but do not establish real ownership transitions. All guarded `real_foreign_*`, root privileged state-trust and cross-mount symlink/watcher scenarios were compiled only. The exact blind candidate's `blocked_mount_transition_log_does_not_hold_state_lock` subprocess regression also passes in this CI log; its source and production dependencies are identical at the candidate.

## For the VM run

Run these **only inside disposable Rocky 8.8 and 8.10 guests**, as root from the checked-out candidate repository, with existing ordinary user `tmtrust`, the pinned toolchain and populated offline cache. These are pending commands, not commands executed by this review. Retain `--nocapture` output and treat every SKIP/early return as untested.

```sh
set -eu
id tmtrust
umask 022
export CARGO_NET_OFFLINE=true
export TIDEMARK_DEPLOY_TEST_VM=1
timeout 1500 cargo test --offline -p tidemark-store --features privileged --test state_trust root_refuses_foreign_owned_ancestry_and_preseeded_regular_database -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemark-cli --features privileged --lib real_foreign_owner_marker_cannot_authorize_root_stage_or_purge -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --lib real_foreign_database_owner_refused_before_daemon_mutation -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6b metrics_refuses_real_foreign_owned_ancestor -- --nocapture
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a1 --test w6a3 --test w6a4 --test w6a6 --test w6a7 --test w6b -- --nocapture
timeout 1500 cargo test --offline -p tidemark-fs --features privileged --test w3b -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-sys --features privileged --test w1c -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a watcher_sixty_second_load_and_ten_minute_fd_stability -- --ignored --nocapture --test-threads=1
```

The live w5a run must actually exercise `filesystem_mark_maps_namespace_bind_and_chroot` on supported 8.10; 8.8's unsupported-filesystem-mark SKIP is evidence only of fallback. Record mode, unmapped counts, directory attribution, CPU, descriptor counts and all cleanup assertions. Add unread-close syscall tracing using the previous audit's fd-provenance design, including reopened handles, failed handle opens and ESTALE; a grep of path strings alone is insufficient.

To run the nonprivileged state regressions as the second user without requiring that user to write the checkout/Cargo cache:

```sh
guest_work=$(mktemp -d /tmp/tidemark-review3.XXXXXX)
chmod 755 "$guest_work"
timeout 1500 cargo test --offline -p tidemark-store --test state_trust --no-run --message-format=json > "$guest_work/build.json"
guest_test=$(python3 - "$guest_work/build.json" <<'PYCODE'
import json, sys
for line in open(sys.argv[1]):
    try:
        item = json.loads(line)
    except ValueError:
        continue
    if item.get('reason') == 'compiler-artifact' and item.get('target', {}).get('name') == 'state_trust' and item.get('executable'):
        print(item['executable'])
PYCODE
)
test -n "$guest_test"
cp "$guest_test" "$guest_work/state-trust"
chmod 755 "$guest_work/state-trust"
timeout 1500 runuser -u tmtrust -- "$guest_work/state-trust" --nocapture --test-threads=1
```

For the missing atime matrix, use a **disposable XFS mount already provisioned by the guest harness** at `/mnt/tidemark-atime`; do not substitute a production mount. The following records actual directory/link inode atimes separately for each mount option:

```sh
guest_mount=/mnt/tidemark-atime
test "$(findmnt -n -o FSTYPE --target "$guest_mount")" = xfs
guest_links=$(mktemp -d "$guest_mount/review3.XXXXXX")
chown tmtrust "$guest_links"
runuser -u tmtrust -- python3 - "$guest_links" <<'PYCODE'
import os, sys
p = sys.argv[1]
open(p + '/target', 'xb').close()
os.symlink('target', p + '/link')
PYCODE
for guest_mode in strictatime relatime noatime; do
    mount -o "remount,$guest_mode" "$guest_mount"
    runuser -u tmtrust -- python3 - "$guest_links" <<'PYCODE'
import os, sys
for p in (sys.argv[1], sys.argv[1] + '/link'):
    os.utime(p, ns=(1000000000, 1000000000), follow_symlinks=False)
PYCODE
    stat -c '%n %X %x' "$guest_links" "$guest_links/link" > "$guest_work/$guest_mode.before"
    timeout 1500 target/debug/tidemark --state-dir "$guest_work/state" symlinks "$guest_links" --check-targets > "$guest_work/$guest_mode.analysis"
    stat -c '%n %X %x' "$guest_links" "$guest_links/link" > "$guest_work/$guest_mode.after"
    diff -u "$guest_work/$guest_mode.before" "$guest_work/$guest_mode.after" || true
done
```

Expected: no link-atime change with noatime; permitted in-mount link-atime changes with strictatime/relatime; directory atime should stay unchanged when NOATIME is effective. The existing privileged cross-mount test must separately preserve the *external* link's old atime. Bind swaps and symlink swaps need concurrent fixture coverage, including the old-kernel mount-ID limitation.

**New regressions still required before resolving R3-01–R3-03:** add deterministic two-uid checkpoints for unmapped-owner ancestry and read-only-bind/writable-alias replacement immediately before SQLite reopen; reject writable/foreign descendants before purge service actions/unlinks; and test watcher byte admission under long names, minute history and multiple mounts. Those tests do not exist under invented test names in this report; the commands above cannot substitute for them. Run the R3-03 paced workload only in a resource-limited guest while observing cgroup memory; no host OOM should be induced. On an equipped runner with cargo-deny and advisory data available, run `timeout 1500 tools/ci.sh --require-deny` and inspect actual merge-required statuses separately.

## What was not checked

No live Rocky 8/XFS/fanotify/SELinux/systemd/LVM/second-user acceptance, actual advisory database run, repository branch-protection inspection, sanitizer/Miri run, 24-hour fuzz campaign or load/OOM experiment was performed. No claim of exploit execution is made for the inferred namespace/read-only-alias scenarios. The copied purge checkpoint is distinguished from production race execution. Third-party unsafe implementations and the unchanged original syscall inventory were not re-audited line by line; the watcher unsafe delta and retained parser ownership are specifically reviewed.

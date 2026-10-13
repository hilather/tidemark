Verdict: CHANGES REQUIRED — **1 BLOCKER / 4 SHOULD-FIX / 1 NIT**.

Reviewed checkout: `fb8021cb14b1c31c2c6a6d46a7b3822bcf191cbb`, from `git rev-parse HEAD` on 2026-10-13. At review start this was the newest commit of the attempt branch and the local `main` and `integration/wave-0` refs. No remote access was used, so remote freshness is not asserted. The later report-only commit does not change the audited source tree.

The retained blind-review brief separately names `CI-shellcheck-severity` candidate `b75b79bac4277a232ad216c44f677304a0d4b758`. Its narrow change was reviewed separately for the required telemetry receipt; it is not substituted for this current-tree security audit. This report's findings and counts refer only to `fb8021c`.

## Scope and method

Read `AGENTS.md`, the W7-C section of `docs/plan/waves/wave-7-hardening-rollout.md`, architecture §8, testing §6, ADR-018/019/020 and contracts R2/R3/R4. The first audit, W6 core/edge reviews and crash/mount/watcher/state fix notes supplied leads; current source, not those notes, is the evidence. The unsafe and content-read inventories were written before starting the privilege/threat-model assessment. Only this report changes the repository. No network, privileged operation, deployment, mount, LVM mutation, branch creation, merge or push was performed. Targeted tests and two temporary-fixture experiments are distinguished from static findings below. These are review evidence and recommendations, not an acceptance or release decision.

No first-party memory-safety defect or scanned regular-file payload read was found in the five metadata consumers. That does not establish an unconditional read-only guarantee: state-path aliases can redirect SQLite reads and writes; symlink inspection can update atime; root capabilities do not provide a read-only sandbox. The installed defaults reduce exposure but do not justify silently accepting unsafe custom paths.

## Findings, most serious first

### F01 — BLOCKER — Direct state access trusts mutable paths; daemon/deploy provenance remains incomplete

Reference: `finding:w7-c1-audit2-f01`.

**Sites:** `crates/tidemark-scan/src/orchestrator.rs:684–699`; `crates/tidemark-store/src/catalog.rs:119–127`, `timeseries.rs:33,50,97`, `writer.rs:32–61`; related `crates/tidemark-cli/src/deploy/files.rs:360–399` and `crates/tidemarkd/src/lib.rs:1244–1299`.

Direct mode creates/opens state by pathname without checking owner, writable ancestors or SQLite leaves. `scan.lock` has final-component NOFOLLOW only. An administrator's `sudo tidemark --state-dir /home/alice/tidemark scan ...` can follow Alice's `timeseries.db` symlink and migrate/write a root-writable file outside Tidemark state; a FIFO can block the header read. A root-owned leaf under an Alice-owned replaceable parent is also unsafe. A local unprivileged fixture calling the **current** `Catalog::open(..., true)` with an empty external file behind that symlink returned success and changed the referent from **0 to 61,440 bytes**, with a SQLite header. This reproduces the vulnerable sink without running sudo or modifying other users' files; the root impact follows from the caller's authority.

Daemon descriptor anchoring now defeats later ancestor redirection of an already-open state fd, and top-level nonregular databases are rejected. However, deployment accepts marker contents as permission to skip state owner/mode checks, and existing children/database files lack owner/private-mode/single-link validation. A normal privileged stage can adopt attacker-preseeded state while the attacker retains writable descendant descriptors. The installed daemon lacks CAP_CHOWN; do not assume it can itself take ownership of a foreign-owned root. The detailed state matrix below distinguishes these cases.

**Change:** introduce one verified, retained state-directory handle in the storage layer, before lock or SQLite access; validate every ancestor's effective replacement authority, require trusted-owned/private state directories and regular/private/single-link files, reject forged-marker ownership bypasses, and keep reads/publication/recovery/removal relative to retained directories. SQLite NOFOLLOW on its main database is defense in depth, not protection for sidecars; trusted nonmutable directories/files are required before SQLite's pathname reopen. Preserve safe euid-owned test fixtures and explicitly handle sticky `/tmp` ancestry. Refuse unsafe preexisting state rather than silently chowning it into apparent trust.

**Files:** new `crates/tidemark-store/src/state_dir.rs` and its `lib.rs` declaration; store `catalog.rs`, `timeseries.rs`, `writer.rs`, `reader.rs`, `space_guard.rs`; scan `orchestrator.rs`; CLI `cmd/scan.rs`, `cmd/snapshots.rs` where it bypasses the shared anchor, and `deploy/{files.rs,mod.rs,purge.rs}`; daemon `lib.rs`; tests in the new helper, `crates/tidemark-store/tests/storage.rs`, `crates/tidemark-scan/tests/w2c/main.rs`, `crates/tidemark-cli/src/deploy/tests.rs`, `crates/tidemarkd/tests/w6a1.rs`. Any frozen public interface addition needs the normal contract review. Exact required checks and adversarial regressions are specified in the state section.

### F02 — SHOULD-FIX — Metrics publication follows an unchecked collector parent

Reference: `finding:w7-c1-audit2-f02`.

**Sites:** `crates/tidemarkd/src/metrics.rs:593,596,624,628`; `crates/tidemark-core/src/config.rs:819` checks only an absolute/nonempty configured path.

`File::open(parent)` follows symlinks and accepts replaceable ancestry. If an administrator configures `/home/alice/export/tidemark.prom`, Alice can rename the collector directory and substitute a symlink immediately before export. Root then writes normal metrics into another root-writable directory under the configured basename. The staging leaf is well hardened, but it does not establish parent provenance. This is a constrained outside-directory overwrite, not arbitrary attacker-chosen bytes. A user-writable collector also permits entry replacement between named-inode validation and publication. Default trusted collector ancestry avoids the scenario. This is a source-backed finding, not an executed exploit.

**Change/files:** in `crates/tidemarkd/src/metrics.rs`, open each collector component with DIRECTORY/NOFOLLOW, validate trusted ownership and absence of untrusted replacement authority using the API's established sticky-parent policy, retain the verified parent, and degrade on rejection without creating/chmoding the collector. Keep current staging checks and descriptor-relative rename. Add symlink, writable-parent, replacement and unchanged-target tests in `crates/tidemarkd/tests/w6b/source.rs` or an explicitly declared test module. No config key change is required.

### F03 — SHOULD-FIX — Process inventory has a count cap but no aggregate pathname-byte budget

Reference: `finding:w7-c1-audit2-f03`.

**Sites:** `crates/tidemark-fs/src/procfs.rs:14,132–136,240–243`; downstream copies in `crates/tidemark-live/src/procscan.rs:139,209,222,312,346`; `packaging/systemd/tidemarkd.service:20,22`.

Up to 1,048,576 per-fd records retain separate owned targets before inode deduplication or exclusion. On a host whose ordinary user/process/fd limits permit it, about 600,000 duplicated read-only descriptors for a roughly 4,000-byte pathname force **2.4 billion bytes (2.235 GiB) of target payload alone**, below the record cap but above the daemon's 2 GiB cgroup limit. Multiple processes can supply this inventory; no API access is required. Reports, phantom holders, samples and queued batches add copies. A smaller inventory can consume headroom while a scan is active. No OOM was induced; this is an allocation lower-bound argument with an explicit host-limit condition.

**Change/files:** enforce a conservative documented aggregate byte budget with fallible reservation in `crates/tidemark-fs/src/procfs.rs`, accounting for vector capacity and retained paths; bound downstream amplification in `crates/tidemark-live/src/procscan.rs`, and return an explicit failed/unavailable pass rather than truncated totals. Add synthetic long-duplicate/phantom/error-preservation tests in `crates/tidemark-fs/tests/w1d/main.rs` and `crates/tidemark-live/tests/w5b/unit.rs`. If the chosen upstream budget cannot conservatively cover queued copies, add payload-byte admission and tests in `crates/tidemarkd/src/services/tswriter.rs`. A documented constant needs no new frozen config key.

### F04 — SHOULD-FIX — Symlink inspection can update symlink atime

Reference: `finding:w7-c1-audit2-f04`.

**Sites:** `crates/tidemark-fs/src/symlinks.rs:75` (`readlinkat_raw`), `:104` (parent NOATIME), `:294` (optional target stat).

Opening the parent with O_NOATIME does not suppress the symlink inode's atime update on readlink. A user can supply old links on relatime/strictatime mounts and make analysis cause metadata writes, contrary to the absolute read-only promise. Target-chain following can also encounter links outside the selected mount. This is **not a regular-file payload read**. A fresh local fixture using a NOATIME parent changed link atime from 1,000,000,000 to 1,791,891,706,674,434,033 ns on kernel `7.2.5-3-omarchy`; no referent contents were opened. Rocky/XFS behavior was not remeasured.

**Change/files:** settle the contract explicitly before implementing a relaxation. Preserving the absolute promise requires declining link-text reads where noatime is not guaranteed, and avoiding target-chain traversal until every traversed mount is known to suppress link atime. Neither O_PATH nor restoring atime afterward is a fix. If that cannot preserve the frozen command behavior, request a narrowly defined exception/restriction through an SCR. Implementation files: `crates/tidemark-fs/src/symlinks.rs`, `crates/tidemark-cli/src/cmd/symlinks.rs`, `crates/tidemark-fs/tests/w3b.rs`; proposed SCR under `docs/handoff/SCR-<allocated-id>.md`. Only after approval, update affected guarantees in `docs/plan/03-link-semantics.md`, `08-interfaces.md`, `12-decisions-and-risks.md` and `docs/security.md`. Guest regressions must separately measure directory/link atimes on strictatime, relatime and noatime mounts. This report does not approve an exception or create an SCR outside its write scope.

### F05 — SHOULD-FIX — The dependency advisory gate remains optional

Reference: `finding:w7-c1-audit2-f05`.

**Sites:** `xtask/src/main.rs:42–46,88–94`; `.github/workflows/ci.yml:14–58`, especially `:21` (`continue-on-error: true`), and `:75`.

The current tree improves on the old finding: cargo-deny is provisioned at a pinned version and checked in a separate job, and `--require-deny` can make its absence fatal. However, ordinary `cargo xtask ci` still warns/skips when deny is missing, and the provisioned workflow job explicitly remains nonblocking. A lockfile with a failing advisory/source/license check can therefore accompany a passing ordinary CI workflow. Repository branch-protection requirements were not inspected. This is a verified gate gap, not evidence of a particular vulnerable dependency. The workflow describes this as a temporary rollout pending resolution of initial findings; the audit does not override that rollout decision.

**Change/files:** resolve or explicitly disposition the existing deny failures, then remove the deny job's `continue-on-error` in `.github/workflows/ci.yml` and ensure its invocation fails on missing tooling. Separately require that status in the repository merge policy if it is intended to gate merges; YAML alone does not configure branch protection. Make the command documented as full CI require deny (or clearly label a separate reduced local check) in `xtask/src/main.rs`, `xtask/README.md` and `tools/ci.sh` if that wrapper supplies the option. Cover missing/failing deny with a fake subprocess in `xtask/tests/w6c.rs`. No advisory refresh, deny pass or policy exception is claimed by this offline audit.

### F06 — NIT — Some human diagnostics bypass the path escaping policy

Reference: `finding:w7-c1-audit2-f06`.

**Sites:** `crates/tidemark-cli/src/deploy/files.rs:401` formats `Path::display()` into an eligibility error; `crates/tidemark-cli/src/cmd/deploy/render.rs:13` and `crates/tidemark-cli/src/lib.rs:167` print diagnostic strings without control-byte escaping.

Normal scanned-name renderers escape controls and invalid bytes, but an administrator-selected deployment/state path containing ESC/newline can reach this error verbatim. A user able to propose such a path can make diagnostic terminal output misleading. Trusted administrator input limits severity; this is not a discovered injection through ordinary scan-result rows. JSON and Prometheus machine-format escaping likewise do not make their decoded/raw contents terminal-safe.

**Change/files:** format path-bearing deployment diagnostics through `display_lossy_bytes` in `crates/tidemark-cli/src/deploy/files.rs`, audit the related `%error`/human rendering boundaries, and ensure `cmd/deploy/render.rs`/`lib.rs` escape control characters at the final human-output boundary without breaking intentional layout. Add hostile-path diagnostic cases in `crates/tidemark-cli/src/deploy/tests.rs` and existing render tests or a focused module. Keep machine JSON encoding unchanged.

## First audit: F01–F06 disposition on this tree

The old report's exact candidate was `8e067aedf76bd7251d9794f4ede17d0715259d9d`, not this tree. Its identifiers in this table must not be confused with the new findings above.

| Old finding | Current status and evidence |
|---|---|
| F01 state trust | **Still holds**, direct mode alone is a reproduced blocker: `orchestrator.rs:684–699`, `catalog.rs:119`, `timeseries.rs:97,50`. Daemon is improved: retained state fd at `tidemarkd/src/state.rs:421–432`, nonregular top-level checks at `lib.rs:1279–1299`; preseeded ownership/marker gaps remain. New F01. |
| F02 config A/B/A race | **Fixed.** `deploy/mod.rs:151–161,300,322,381` parses, hashes and installs one captured byte vector; `deploy/purge.rs:110–111`, `deploy/files.rs:165–166` and daemon `lib.rs:1334–1372` use captured-byte parsing. Replacement regression tests are present at `deploy/tests.rs:859,1012`; not rerun here. |
| F03 symlink atime | **Still holds.** `tidemark-fs/src/symlinks.rs:75,104,294`; fresh local probe reproduced the syscall property. New F04. |
| F04 scan-setup ancestor follow / ungated retry | **Fixed in current setup path.** `tidemark-scan/src/orchestrator.rs:725–733` delegates to the component opener in `tidemark-fs/src/walk/getdents.rs:38,74`; each attempt/retry charges the gate (`:63–68`). Setup ancestor-symlink regression exists at `orchestrator.rs:1456`; not rerun here. General EL8 bind-race limits remain separate. |
| F05 invalid unit memory values | **Fixed.** `packaging/systemd/tidemarkd.service:20,22` are plain `2G` values. `tests/install/install.sh:118–126` checks effective 2147483648-byte memory/capability settings, and :146 checks enforcing SELinux. The supplied Rocky guest facts are consistent; no guest validation was rerun. |
| F06 optional deny | **Partially mitigated, still holds.** Current pinned workflow and strict option exist; `xtask/src/main.rs:88–94` still allows a skip and `.github/workflows/ci.yml:21` is nonblocking. New F05; the intentional rollout qualification matters. |

## Unsafe audit of `fb8021cb14b1c31c2c6a6d46a7b3822bcf191cbb`

Static review found **20 unsafe blocks: 17 production, 3 test**, all under `crates/tidemark-sys`. There are no first-party unsafe functions, implementations, traits or extern blocks, and no executable unsafe occurrence elsewhere in the workspace. Other `unsafe` matches are comments, diagnostic strings and lint attributes. The non-sys library/binary roots, including `test-support` and `xtask`, explicitly forbid unsafe; their manifests inherit the workspace `unsafe_code = "forbid"` at `Cargo.toml:74`, covering integration tests as well. The sys manifest denies unsafe by default, with local allowances in its three implementation modules and privileged test. Every block has a nearby SAFETY comment. No concrete memory-safety defect was established in the reviewed first-party wrappers.

This conclusion assumes the Linux syscall ABI, including valid newly allocated descriptor numbers from fanotify, is trustworthy. It does not claim resilience to an actively hostile kernel. All files under `crates/tidemark-sys`, the W7-C section, architecture §8, testing §6, ADRs and contract R2/R3/R4 were read. The parent audit ran `timeout 1500 cargo test --offline --locked -p tidemark-sys`: 13 tests passed, zero failed; layout and descriptor-closure tests executed. No Miri, sanitizer or privileged operation ran.

In the table, `sys/` means `crates/tidemark-sys/`; each row is one unsafe block. PASS refers to the actual call graph at this commit, not arbitrary future callers of private helpers.

| Site | Operation and verified safety conditions | Verdict |
|---|---|---|
| `sys/src/xfs.rs:19` | `fstatfs` gets a borrowed live descriptor and correctly aligned `MaybeUninit<libc::statfs>` output of the proper size. Failure returns without assuming initialization. No contents are read. | PASS; comment states buffer and lifetime/error conditions. |
| `sys/src/xfs.rs:23` | `assume_init` occurs only after successful `fstatfs`, whose ABI initializes the structure. Padding need not be initialized to construct/read the integer fields. | PASS under the syscall success contract. |
| `sys/src/xfs.rs:54` | Geometry ioctl writes a live `repr(C)` 112-byte, 8-aligned object. Struct fields are initialized by `Default` before the call, and only a successful ioctl is interpreted. A failed/partial syscall leaves no uninitialized-field read. | PASS; measured size is corroborated by layout tests in source. |
| `sys/src/xfs.rs:200` | Legacy bulkstat request is a correctly sized pointer-bearing C struct; `last`, `count` and the `MaybeUninit<xfs_bstat>` array are exclusively live through the synchronous ioctl. Its production caller supplies exactly one slot, so `buffer.len() as i32` is lossless. Negative/oversized returned counts are rejected at 201–203; legacy records are never decoded. | PASS for the current private call graph. The unchecked cast and allocation precondition must be revisited if this helper gains arbitrary callers. |
| `sys/src/xfs.rs:214` | v5 ioctl receives the initialized, 8-aligned `u64` allocation produced by its only production caller, `request_v5`. Probe uses 32 words; iteration uses `8 + 24 * batch` words after the `1..=65536` check. No pointer escapes the synchronous call. | PASS; the SAFETY comment correctly delegates allocation invariants to `request_v5` callers. |
| `sys/src/xfs.rs:237` | Typed header write occupies the first 64 bytes of that live allocation. `xfs_bulk_ireq` has 8-byte alignment and all-integer fields, with no internal padding at the asserted offsets. Reserved words and output count are zeroed by `Default`. `batch as u32` is lossless for probe/validated iterator callers. | PASS. The helper itself accepts a slice; sufficient capacity is established by every current call site. |
| `sys/src/xfs.rs:242` | Typed header read follows a successful ioctl. Header storage remains initialized/live/aligned even if the kernel returns no records. `ocount > batch` is rejected before any record read. | PASS. |
| `sys/src/xfs.rs:323` | Record offset is `64 + 192*i`, aligned to 8; validated count never exceeds the allocation. The entire `Vec<u64>` is initialized before first ioctl and contains only valid integer bit patterns for the C record. Short batches use only reported records; zero count finishes. Failed ioctl skips decode. | PASS. Partial kernel writes cannot produce Rust UB, though falsely reported successful counts/partial fields could produce wrong metadata under a broken kernel ABI. |
| `sys/src/ioprio.rs:18` | `syscall(SYS_ioprio_set, PROCESS, 0, encoded_priority)` has integer-only arguments. The public best-effort setter restricts priority to 0–7; idle uses level zero. Error `-1` preserves errno. `who=0` targets the calling thread. | PASS; no raw pointer or ownership concern. The source comment reverses the names of `which` and `who`; actual syscall arguments are correct (`which=PROCESS`, `who=0`). |
| `sys/src/fanotify.rs:88` | Integer-only `fanotify_init` requests notification mode, nonblocking/CLOEXEC group and read-only/CLOEXEC event descriptors. Result is checked for failure before adoption. Numeric report/init flags agree with local Linux headers. | PASS. Event descriptors permit reads at the OS level, so the higher-level no-content-read rule remains an application invariant. |
| `sys/src/fanotify.rs:99` | Adopts the successful newly returned group descriptor exactly once, into private `OwnedFd` storage. | PASS. |
| `sys/src/fanotify.rs:113` | `read` writes only to the supplied mutable slice with its exact byte length; descriptor is the private fanotify group, never an event descriptor. Error `EAGAIN` becomes an empty iterator; other errors propagate. Positive count narrows to the returned prefix, excluding stale buffer tail. | PASS under `read`'s `0 <= n <= len` contract. An impossible oversized return would cause a safe slice panic, not unchecked memory access. The comment could additionally spell out that count contract. |
| `sys/src/fanotify.rs:129` | `fanotify_mark` receives a `CString` that rejects interior NUL and remains live through the call, plus the borrowed group descriptor. Integer flags include `FAN_MARK_DONT_FOLLOW`; errors preserve errno. This concerns the final symlink; ancestor trust is a caller-level path-boundary question. | PASS for memory safety. |
| `sys/src/fanotify.rs:191` | Copies the 8-byte `file_handle` header and at most 128 handle bytes into `[u64;17]` (136 initialized bytes, 8-aligned). `len as u32` is bounded; temporary `to_ne_bytes` arrays live through each copy, and caller input cannot alias the new local storage. Zero-length copies retain valid slice pointers. | PASS; comment states capacity, preceding line states alignment. |
| `sys/src/fanotify.rs:197` | `open_by_handle_at` sees the initialized/live 8-byte header plus bounded tail, with correct alignment. It uses `O_PATH\|O_CLOEXEC`, so the returned descriptor cannot read payload data. Kernel validates handle type/value and mount descriptor; errors are returned. | PASS. |
| `sys/src/fanotify.rs:208` | Adopts one newly returned descriptor only after nonnegative syscall success. | PASS. |
| `sys/src/fanotify.rs:284` | Adopts a nonnegative event descriptor from bytes produced only by the private fanotify-group reader in production. A minimum 24-byte header is checked before accessing the field. Each decoded record advances once; iterator drop consumes structurally traversable remaining records. Adoption precedes semantic validation so the current event descriptor closes on invalid version/length as well. | PASS under documented kernel-origin/unique-descriptor invariant. This decoder is not safe as a public arbitrary-byte fuzz entry point; see parser guidance below. |
| `sys/src/fanotify.rs:487` | Test-only `fcntl(F_GETFD)` verifies closed integer descriptor and `EBADF`; no pointer or ownership transfer. The descriptor lifetime test runs in its own child to avoid descriptor-number reuse by parallel test threads. | PASS. |
| `sys/tests/w1c/main.rs:100` | Privileged-test `statx` uses a live NUL-terminated pathname and aligned zeroed output, NOFOLLOW/NO_AUTOMOUNT flags, then asserts success before any assume-init. | PASS. |
| `sys/tests/w1c/main.rs:111` | Test-only `assume_init` follows successful `statx`; buffer was already zeroed and integer fields have valid bit patterns. It then verifies requested mask availability before treating fields as measured. | PASS. |

### ABI/layout cross-check

`sys/src/gen/bindings.rs` declares each kernel structure `repr(C)`. Tests at 187–198 assert size, alignment and every field offset of `xfs_bstime` (16/8), `xfs_bstat` (136/8), `xfs_fsop_bulkreq` (32/8), `xfs_fsop_geom` (112/8) and `xfs_fsop_geom_v1` (112/8). Tests at 230–246 assert every v5 offset for `xfs_bulk_ireq` (64/8), `xfs_bulkstat` (192/8), and the flexible-array request header (64/8, records offset 64). The actual typed reads at `xfs.rs:237,242,323` agree with these tested 64-byte/192-byte strides. The legacy pointer request's four-byte padding after the 32-bit `icount`, before the 8-aligned output pointers matches its asserted 32-byte layout; it is never treated as an integer byte buffer.

The ioctl tests at `bindings.rs:201–227,234–242` assert the exact request constants and encoded sizes: geometry-v1 `0x80705864`, geometry `0x8070587c`, legacy bulkstat `0xc0205865`, and v5 `0x8040587f`. Runtime uses only geometry-v1, legacy probe and v5 bulkstat. Source provenance identifies Rocky 8 x86_64 gcc measurements for legacy structures and Rocky 9 v5 headers because the stated EL8 userspace header lacks v5. I checked internal agreement with these recorded layouts, not a newly executed C/Rust layout experiment; no XFS UAPI header suitable for an independent host comparison was present. Guest ioctl behavior is therefore an external trust assumption, not newly reproduced evidence.

The manually decoded fanotify offsets at `fanotify.rs:272–274` agree with the host `/usr/include/linux/fanotify.h` definitions read during this audit: metadata fields 0/4/6/8/16/20, metadata size 24; information header size 4; fsid at 4; handle header at 12, type at 16, payload at 20. The local `/usr/include/bits/fcntl-linux.h:365–370` agrees with the 8-byte `file_handle` header. These are host header cross-checks, not fresh Rocky guest layout evidence. No fanotify alignment assumption is needed in byte decoding; it uses indexed bytes after length checks.

### Error, partial and oversized outputs

* XFS iterator bounds `batch` before arithmetic/allocation and narrows it losslessly to `u32`. Legacy `icount` has only the one-slot production caller. A v5 count larger than capacity is rejected before typed record access, legacy rejects negative or oversized counts, and every syscall error propagates without parsing output. Record storage is initialized, so a short/partial write cannot create invalid Rust scalar representations. A short successful batch does not imply end-of-stream; only zero records ends it. Cursor progress, ascending inode order, lower range and upper cutoff checks prevent endless replay or duplicate publication (`xfs.rs:332–357`). Checked multiplication and conversion reject overflowing block counts and wide xflags (`:250–266`).
* Fanotify short/truncated headers, invalid event lengths and oversized information/handle lengths produce `InvalidData`. Event length must be at least 24 and fit the read prefix; metadata length fits the event; info records must be at least four and fit the remainder; handles are capped at 128; named records need a terminating NUL. The current event fd closes on semantic validation failure. An invalid event boundary discards the remainder, so the iterator's cleanup guarantee covers well-formed kernel record boundaries; it cannot reconstruct arbitrary corrupt boundaries and promise to recover every embedded integer as a descriptor. No local-user path to invent the raw stream was found. The ordinary kernel buffer is private to `Fanotify::read` and the iterator's private field.
* Existing tests exercise fanotify truncations, invalid sizes, both FID variants, descriptor close on error/abandoned batches; XFS tests cover invalid public batch sizes, unsupported v5/errno preservation, numeric conversion overflow, order and range errors. The source does **not** add a mocked successful-v5 oversized-count/short-write test. Adding such a regression test would improve evidence but absence alone is not a memory-safety finding.

### Miri/sanitizer candidates

`ag_ino_range`, block conversion, `convert_v5`, and `finish_batch` can run under Miri after avoiding the incidental `File::open("/")` in iterator fixtures or using a controlled descriptor. The `V5_MOCK` seam can exercise header/record casts with an initialized synthetic `Vec<u64>` and boundary counts without executing ioctl. Layout tests and integer priority encoding are also pure. These are strong candidates for Miri/UB sanitization of the custom raw-buffer manipulation.

Fanotify length/FID parsing is byte-only except for `OwnedFd` adoption. Run existing synthetic cases whose `fd == -1`, or extract a pure validated event view before a separate trusted fd-adoption step and fuzz that pure parser. **Do not feed arbitrary positive fd integers into the current `decode`**: constructing/dropping `OwnedFd` can close unrelated process descriptors, and a duplicate/invalid number violates its safety assumptions. To test ownership separately, supply only deliberately opened unique descriptors as the existing child test does. Real ioctl/fanotify syscalls need native Linux/VM tests rather than Miri; a native sanitizer likewise does not instrument the kernel. No dynamic tool result is claimed here.

## Content-read audit (current HEAD fb8021cb14b1c31c2c6a6d46a7b3822bcf191cbb)

Static result: no scanned regular-file payload read was found in the walker, bulk scanner, process scanner, watcher or symlink analyzer. This result is limited to those metadata consumers. It is **not an unconditional guarantee for the CLI/daemon process**: operator configuration, host OS metadata, SQLite state, the dynamic loader, procfs/sysfs and notification streams are legitimate reads. Unsafe state aliases can cause those state reads to reach scanned files; see the direct-state finding. This was source inspection, not a guest syscall trace.

Searched production and test sources for `read`, `pread`, `readv`, `read_to_end`, `read_to_string`, `BufRead`, `mmap`, `sendfile`, `copy`, `readlink` and directory readers, then followed descriptor creation and caller paths. No first-party payload `pread*`, `readv`, `sendfile`, `copy_file_range`, `std::fs::copy` or `std::io::copy` exists in the five production scanner call graphs. Tests' fixture/oracle reads do not run in shipped consumers. XFS pointer `.read()` copies an initialized ioctl memory record, not file contents; `AttributionCache::read` is a metadata lookup; `RwLock::read` is synchronization. SQLite can perform VFS reads/mappings below Rust call sites, so source grep alone is insufficient for a global negative assertion.

### Read-site inventory

Paths in the table are relative to `crates/`; multiple callers of a shared read helper are explicit.

| Current source site | Read / provenance | Classification and guarantee |
|---|---|---|
| `tidemark-fs/src/walk/worker.rs:434,447` | `RawDir` iterator refill, using fd from `getdents::open_directory` at :324 | `getdents64` directory names only. `getdents.rs:89-95,113` requires `O_DIRECTORY\|O_NOFOLLOW`; ancestors are `O_PATH`, final dir `O_RDONLY`; `O_NOATIME`/EPERM fallback. Opened identity/device/available mount ID are checked at worker :344-395. Entries are statx metadata only (:516,:588), never opened for data. |
| `tidemark-fs/src/walk/getdents.rs:170,180` | `read_fd` RawDir refill | Directory only. Unchecked public reader delegates through `open_directory` (:158-161). Bulk phase-3 name resolution calls `read_dir_entries_checked_cancellable` from `tidemark-scan/src/bulk/names.rs:79`; checked helper requires root containment, component no-follow directory opens, device/available mount ID and expected final inode before enumeration (:229-261). |
| `tidemark-fs/src/mount.rs:193-195` | Open fixed `/proc/self/mountinfo`, `take(16 MiB + 1).read_to_end` | Kernel mount table. Walker :99, symlink analyzer :210 and scan orchestrator :130 use it. No entry name controls this path; assumes trusted host procfs. |
| `tidemark-fs/src/procfs.rs:24-26`, caller :65 | `bytes("/proc/{pid}/comm")`, capped 1 MiB + sentinel | Kernel process metadata. Typed numeric PID and fixed suffix; comm contents never become a pathname to open. |
| `tidemark-fs/src/procfs.rs:80,84` | Open `/proc/{pid}/stat`, single `read` into 512 bytes | Kernel starttime metadata. Full or non-newline-terminated record rejected (:87); process identity checked around descriptor enumeration and attribution. This implementation has changed since the first audit's generic-text-helper inventory. |
| `tidemark-fs/src/procfs.rs:24-26`, caller :125 | `text("/proc/{pid}/cgroup")` | Kernel cgroup metadata, 1 MiB + sentinel. Parsed unit text never opened as a file. |
| `tidemark-fs/src/procfs.rs:24-26`, caller :147 | `text("/proc/{pid}/fdinfo/{fd}")` | Kernel fd flags, typed PID/fd, 1 MiB + sentinel. **fdinfo** is opened, never the `/proc/PID/fd/N` target. |
| `tidemark-fs/src/procfs.rs:24-26`, caller :151 | `bytes("/proc/{pid}/status")` | Kernel UID metadata, 1 MiB + sentinel. |
| `tidemark-fs/src/procfs.rs:37`, callers :61,:129 | `fs::read_dir` on `/proc` and `/proc/{pid}/fd` | Procfs directories only; numeric entry IDs, sorted, maximum 1,048,576 IDs. |
| `tidemark-fs/src/procfs.rs:133` | `read_link("/proc/{pid}/fd/{fd}")` | Procfs magic-link text only. `scan_open_files` intentionally follows fd magic links with `metadata` (:209,:232), checks regular-file/device and rechecks `(dev,ino)`, but never opens the target. Process scanner :53 delegates here and attribution :106-113 reads only the fixed proc metadata above. |
| `tidemark-sys/src/fanotify.rs:113`, caller `tidemark-live/src/watch/mod.rs:430` | `libc::read` on private `Fanotify.fd` produced by `fanotify_init` (:89) | Kernel notification stream, **not event-file contents**. Fd mode read buffer is 16-256 records × 24 bytes; richer modes use 65,536 bytes (`watch/mod.rs:360-367,424-431`). Event fd is separate from group fd. |
| `tidemark-live/src/watch/fdmode.rs:147` | `read_link("/proc/self/fd/{event fd}")` | Kernel descriptor-path text. `resolve_event` :112 uses `statx(AT_EMPTY_PATH)`, falling back to `fstat` :130. No `read` on event fd and no open of resolved path. |
| `tidemark-live/src/watch/mod.rs:531-533` | `/proc/self/fdinfo/{retained mount fd}`, bounded `read_to_string` | Mount ID metadata, 16 KiB + sentinel. Mount fd itself is fstat-ed, not read as a data file. |
| `tidemark-live/src/watch/mod.rs:710` (two calls) | `read_link("/proc/self/ns/mnt")` and `/proc/1/ns/mnt` | Namespace identity text only. |
| `tidemark-live/src/watch/fidmode.rs:17,29,40` | Handle fd from `sys/fanotify.rs:198`, then fdmode `readlink`/fstat/statx | `open_by_handle_at` uses `O_PATH\|O_CLOEXEC` (:201): descriptor cannot read file payload. Named metadata uses `statat`/`statx` `SYMLINK_NOFOLLOW` (:50-55), never open of entry name. |
| `tidemark-fs/src/symlinks.rs:234,243` | `RawDir` enumeration | Directory metadata. Component opens at :101-108 use `O_DIRECTORY\|O_NOFOLLOW\|O_NOATIME` (EPERM fallback), opened identity/mount checked at :229-233. |
| `tidemark-fs/src/symlinks.rs:75` | `readlinkat_raw` through verified directory fd and raw entry name | Symlink target bytes only; buffer 256 bytes growing to 64 KiB. A raced-in regular file fails readlink rather than reading payload. Optional target check is `statat` (:294) with intentional symlink resolution, **not** a payload open/read. Symlink atime effects remain a separate read-only concern. |
| `tidemark-fs/src/blockdev.rs:30-32`, callers :77,:84,:123,:130,:206 | Bounded sysfs `read_to_string` for `dev`, `md/level`, `queue/rotational`, `queue/scheduler`, `stat` | Production base fixed `/sys/dev/block`; major/minor typed numbers. Canonicalization intentionally follows sysfs topology links (:65). 64 KiB + sentinel. Orchestrator :134,:140 and adaptive Gate use these reads, not scanned entries. |
| `tidemark-fs/src/blockdev.rs:94`; `tidemark-fs/src/throttle.rs:536` | `read_dir` on device `slaves` and `/sys/dev/block/{major}:{minor}/mq` | Sysfs topology directories; graph capped 4,096 nodes/edges. |
| `tidemark-scan/src/orchestrator.rs:771-773` | Fixed `/proc/thread-self/status`, bounded `read_to_string` | Effective capability metadata, 64 KiB + sentinel. |
| `tidemark-scan/src/orchestrator.rs:85,1239-1241` | `File::open("/etc/os-release")`, bounded `read_to_end` | Intentional host OS configuration (16 KiB max), not selected based on scanned directory entry. Ordinary open can follow the distribution's os-release link; assumes trusted system configuration. New read relative to first audit's old tree. |
| `tidemark-scan/src/orchestrator.rs:1274-1276` | Fixed `/proc/self/status`, `take(65_536).read_to_string` | Peak RSS metadata, bounded. |
| `tidemark-core/src/config.rs:681-683` | Operator-selected TOML `File::open` / `read_to_end` | Explicit CLI config, 1 MiB + sentinel. Follows symlinks; not derived from a scanned entry, but trust of that input is not established by this helper. |
| `tidemarkd/src/lib.rs:1334-1358` | Daemon config `openat(RDONLY\|NOFOLLOW\|CLOEXEC\|NONBLOCK)` then bounded read | Explicit configuration. Parent opened componentwise; fd type required regular before read; exact captured bytes parsed and hashed. 1 MiB + sentinel. |
| `tidemark-store/src/space_guard.rs:39-41` | `/proc/self/fdinfo/{directory fd}`, bounded `read_to_end` | Mount metadata, 16 KiB + sentinel. |
| `tidemark-store/src/space_guard.rs:77,79` | `Dir` refill for own-state usage | Directory entries only. Child handles `O_PATH\|NOFOLLOW`, fstat; no payload read. |
| `tidemark-store/src/timeseries.rs:97-101` | Ordinary `File::open(path)`, up to 16-byte header read | Own-state database by intent. Also used by catalog, whose actual filename is `state_dir/timeseries.db` (`catalog.rs:127`). This open follows symlinks; it does not itself prove ownership/confinement. A malicious state alias can violate payload isolation. |
| `tidemark-store/src/timeseries.rs:50`; `reader.rs:173`; `writer.rs:61` | SQLite VFS reads/journal/WAL/mappings underneath rusqlite | Own catalog/timeseries/snapshot state by intent. Snapshot immutable/read-only opening still reads bytes. Writer exclusively creates tmp then reopens by pathname. Protected ancestry/leaf identity is essential. SQLite diff explicitly enables mappings (`diff.rs:305,307`) for state snapshots, outside scanner core. |
| `tidemark-store/src/catalog.rs:281,286` | `fs::read_dir` for snapshot rebuild | Own-state directories. Subsequent snapshot opening is SQLite state read, with the state-boundary qualifications above. |

Adjacent production reads that are not descendants of the five metadata consumer entry points: `tidemark-fs/src/probe.rs:87-89` reads fixed host `/etc/os-release`, `/etc/rocky-release`, `/proc/meminfo`, `/proc/self/status` through a 1 MiB cap; `tidemark-fs/src/lvm.rs:205` reads subprocess stdout/stderr pipes (bounded), not file payload; `tidemark-cli/src/client.rs` uses `BufReader<UnixStream>` for the Unix API; `tidemarkd/src/api.rs:321` reads that socket. Deploy/firstboot reads configuration and own JSON artifacts through their helpers; their boundary checks belong to the deployment review. `tidemarkd/src/services/sampler.rs:36` reads fixed numeric `/proc/self/fdinfo/N` for mount identity. The runtime will also read system loader/library metadata; the guest oracle must classify it separately.

### Fanotify unread-close reasoning

`sys/fanotify.rs:283-286` adopts each nonnegative event fd into an `OwnedFd` before validation, so rejected valid-length metadata records and overflow records drop that fd. `EventIter::drop` drains remaining records (:260-264). Watcher collects one bounded batch into a `Vec<Result<RawEvent>>`; stopping inside the loop drops the unprocessed owned events (`watch/mod.rs:430,446-455`). Own-PID early returns also drop the event fd. Normal fd-mode handling borrows the fd only for statx/fstat and procfs readlink (:764-769), returns copied metadata/path, and drops the owned fd at match-arm completion before recording/attribution. FID O_PATH handles likewise drop after resolving metadata. No content read in these paths.

This lifecycle argument assumes valid kernel framing. A fabricated/truncated record whose length cannot frame the rest causes the iterator to discard the remainder, so do not claim a safe parser accepts arbitrary bytes containing arbitrary live descriptor numbers. Fuzz the pure length/FID parser with fd=-1, or separate ownership adoption from parsing. The underlying source code supports unread closure for the actual kernel batches; no syscall trace was executed here.

### Guest strace regression design (not implemented or executed)

Add a guest-only fault script (e.g. `tests/fault/07-content-reads.sh`) and a checker (e.g. `tests/fault/content_reads.py`), following the existing explicit `TIDEMARK_DEPLOY_TEST_VM=1`, root, cleanup and deadline conventions. Use prebuilt `--features privileged` binaries, not Cargo beneath strace. Run once on each disposable Rocky 8.8/8.10 VM in its host mount namespace, because the watcher deliberately refuses a private mount namespace. A setup harness creates a disposable XFS fixture, verifies v5/ftype eligibility, and creates root-owned 0700 work/config/state/traces **off the scanned XFS filesystem**. The existing fault common setup puts state on its test XFS and must be adjusted for this particular script. Never use production mounts/config/state.

Before tracing, create and record `(dev,ino)` for nonempty regular and sparse files, hard-link aliases, regular-file and directory symlinks, an outside sentinel, dangling and looping symlinks, and names containing LF/TAB/ESC/invalid UTF-8. A separate untraced helper keeps an unlinked file open and grows an O_WRONLY live file; it must explicitly close/reopen other write fds to generate CLOSE_WRITE. Use readiness/control pipes and bounded waits. Fixture setup/copy/hash reads remain outside the traced process tree. Use a quiescent fixture for walk/bulk; run rename-to-symlink race trials separately with their own expected refusal/degraded outcomes.

The harness sets `AUDIT_MOUNT` to the disposable mount root (so forced bulk is eligible), `AUDIT_BIN` to the absolute prebuilt CLI binary, and `AUDIT_WORK` to its private root-owned workdir, then writes TOML with properly escaped paths:

```toml
[general]
state_dir = "<AUDIT_WORK>/state"
socket = "<AUDIT_WORK>/absent.sock"
[[roots]]
path = "<AUDIT_MOUNT>"
schedule = "off"
mode = "walk"
throttle = "max"
[scan]
threads = 2
memory_budget = "1280MiB"
[watch]
mounts = ["<AUDIT_MOUNT>"]
[metrics]
textfile = ""
[lvm]
enabled = false
```

Exact tracing invocations, after setup and helper readiness:

```bash
set -euo pipefail
export LC_ALL=C
trace_case() {
    local case_name="$1"
    shift
    strace -ff -ttt -yy -xx -s 131072 -v \
        -e trace=%file,%desc,%process,%memory \
        -o "$AUDIT_WORK/$case_name.trace" \
        "$AUDIT_BIN" -c "$AUDIT_WORK/config.toml" --json "$@" \
        >"$AUDIT_WORK/$case_name.json" 2>"$AUDIT_WORK/$case_name.stderr"
}
trace_case walk scan "$AUDIT_MOUNT" --mode walk --threads 2 --throttle max
trace_case bulk scan "$AUDIT_MOUNT" --mode bulk --threads 2 --throttle max
trace_case phantom phantom --direct --mount "$AUDIT_MOUNT"
trace_case writers writers --direct --mount "$AUDIT_MOUNT" --interval 2s
trace_case symlinks symlinks "$AUDIT_MOUNT" --check-targets --limit 100
trace_case watch watch --mount "$AUDIT_MOUNT" --events close_write,modify &
watch_trace_job=$!
# Controller observes successful fanotify mark + watcher event readiness,
# asks the untraced helper to emit the required close-writes, checks JSON,
# then sends SIGTERM to the traced tidemark PID from execve, not strace.
wait "$watch_trace_job"
```

The future controller must impose 120-second deadlines per case, kill/reap only its own trace tree on failure, and preserve traces, status, inode manifest and narrowly defined allowlist. Unsupported strace options or absent capabilities are infrastructure failure/pending, never PASS. Confirm snapshot metadata reports `walk` and forced `bulk` respectively, bulk trace contains a successful v5 ioctl, phantom reports the known deleted holder, writers reports live growth, symlink output exercises outside/loop cases, and at least one non-self close-write event appears in watch JSON. Observe mark success **and** repeated helper events until output so the race between mark and `enable_events` cannot create an empty false pass. A second watcher trial signals shutdown with a nonempty queued batch, verifying unread fd cleanup. Run a complete daemon startup/scan/watcher trial under the same oracle as an additional integration case; its own-state/config/runtime allowlist remains explicit.

Checker assertion and required implementation:

1. Parse unfinished/resumed records and per-PID timestamps; maintain fd tables shared by `clone(CLONE_FILES)` or copied by fork, cwd/dirfd references, `open/openat`, dup/fcntl duplication, close/reuse, exec CLOEXEC, exits and mappings. `-yy` paths are supporting evidence, not the only identity source.
2. Identify notification-group fd by successful `fanotify_init`; decode **all bytes** returned from successful group reads using native-endian 24-byte fanotify headers. Every nonnegative event fd enters the fd table even though it has no `openat` event. Current fd batches max 6,144 bytes; richer batches 65,536 bytes; 131,072 string limit exceeds both. Reject truncated/unparseable required payload/trace, and unsupported descriptor transfer or io_uring payload routes. Require explicit close of each delivered event fd before normal process exit (not merely exit cleanup).
3. Reject **every attempted** `read/readv/pread64/preadv/preadv2`, sendfile/copy_file_range/splice input, and file-backed readable/executable mmap on a fixture payload inode or event fd, including calls that fail/return zero. Reject unclassified regular-file sources. Also reject data-capable opens of scanned regular files by walker/bulk; `O_PATH` metadata handles are allowed. Bind `(dev,ino)` using the fixture manifest and observed fstat/statx; follow dup/inheritance/aliases/deleted names, not just string prefixes. File-backed PROT_NONE mappings need tracking through later mprotect.
4. Permit only specifically classified directory getdents, symlink readlink/readlinkat, stat/ioctl metadata, group-stream reads, fixed numeric procfs metadata suffixes, numeric sysfs metadata, exact config file, protected private state (SQLite sidecars), and independently identified root-owned runtime loader/library files plus `/etc/ld.so.cache` and `/etc/os-release`. Do not blanket-allow `/proc` (fd magic links can read payload), `/etc`, `/tmp`, `.db` suffixes, or all subprocesses. Fixture inode membership overrides allowlists, catching aliases into a nominally permitted location.
5. Fail if required coverage is absent. Store source classification counts, event fds received/explicitly closed, prohibited access count (must be zero), coverage observations and command status. Test the oracle itself using a deliberately reading helper, a mmap/mprotect reader, dup/inheritance, hard-link aliases, fd reuse after close, and truncated traces; forbidden examples must fail while legitimate reused config fds pass.

This tests actual executed behavior; it cannot prove safety against privileged kernel/mount manipulation or repair state/config ancestry. This guest design was not implemented or run; the native tests performed by this audit are listed below.

## State and deployment boundaries at `fb8021cb14b1c31c2c6a6d46a7b3822bcf191cbb`

### F01 remains a BLOCKER, with a narrower daemon scenario than the old report

**Primary locations:** `crates/tidemark-scan/src/orchestrator.rs:684`, `crates/tidemark-store/src/catalog.rs:119`, `crates/tidemark-store/src/timeseries.rs:33,50,97`, `crates/tidemark-store/src/writer.rs:32,61`; related incomplete provenance checks in `crates/tidemark-cli/src/deploy/files.rs:15,360,372,399` and `crates/tidemarkd/src/lib.rs:1244,1279`.

Direct scans still trust state pathname ancestry and preexisting SQLite leaves. The CLI's configuration validator checks only that `general.state_dir` is absolute (`tidemark-core/src/config.rs:748`), then direct-mode preflight immediately acquires the lock and opens/recovers the catalog (`tidemark-cli/src/cmd/scan.rs:156–168`). `ScanLock` recursively creates pathname directories with requested mode 0700 but leaves existing directories alone; its `O_NOFOLLOW` applies only to `scan.lock`. It does not check ancestry ownership/modes, lock type/owner/link count, or the state directory's identity. `Catalog::open` repeats pathname directory creation and delegates to the ordinary `timeseries.db` path. Header preflight follows symlinks with `File::open`, and SQLite is separately opened by path with no `SQLITE_OPEN_NOFOLLOW`. Snapshot construction likewise creates its directory by path, exclusively creates a temporary file, closes it and reopens its name with SQLite.

**Concrete scenario:** an administrator runs a daemon-absent `sudo tidemark --state-dir /home/alice/tidemark scan ...`, or a configured state path is below an Alice-writable parent. Alice can precreate `timeseries.db` as a symlink to a root-owned empty file or compatible SQLite database that root can write. No race is needed: `try_exists` follows it, header preflight accepts an empty file or SQLite header, SQLite opens the referent for writing, and a version-0 referent is migrated at `timeseries.rs:88`; later catalog calls insert/update scan state. That crosses the state-only write boundary. A FIFO in place of the database can instead block `File::open` in header preflight; `ScanLock` itself can open a FIFO read-write without checking its type. A live attacker can replace path components between the successive lock/catalog/snapshot calls; the final no-follow lock flag does not protect ancestors or subsequent calls. A initially private root-owned leaf below an attacker-owned non-sticky parent is still renameable by that parent owner.

**Daemon improvement verified:** `prepare_state` now retains a component-wise no-follow state directory; it checks top-level timeseries main, WAL, SHM and migration-backup names with `statat(...SYMLINK_NOFOLLOW)` and rejects nonregular entries (`tidemarkd/src/lib.rs:1279–1299`). `LiveState::storage_path` supplies `/proc/self/fd/<n>/.` (`state.rs:421–432`); catalog, sample writer and scheduler use that anchor (`lib.rs:661–714`, `services/tswriter.rs:82,128`, `scheduler.rs:708`). Renaming an ancestor does **not** redirect that already-retained daemon descriptor. Do not copy the old daemon-symlink scenario without this qualification.

**Remaining daemon/deploy provenance gap:** `files::directory` walks components with NOFOLLOW but does not check whether another user owns or can rename a component. `state_handles` calls `marker_authorizes` before `eligible`; a marker with the right format and lexical path makes `eligible(..., marked=true)` skip both owner and mode checks. The marker reader checks regular type and size, but not marker owner, mode or link count. Consequently content written by an unprivileged directory owner can authorize a normal `sudo deploy stage` (with its usual root capabilities) taking ownership of that preseeded state tree. `complete_handles` changes only the state root; it does not establish ownership of preexisting SQLite files or snapshot descendants. An attacker can retain an open writable regular file or directory descriptor before that normalization. Existing snapshot/tmp directories receive path/mount checks, not `eligible`; arbitrary existing snapshot-root descendants are not checked. Top-level database validation checks type only, not uid/mode/nlink, and omits the possible rollback-journal name. Therefore a preseeded foreign-owned regular database, hard link, or foreign-owned snapshot directory remains writable through the attacker's retained descriptor even after the top-level directory becomes private. A marker is identification data, not evidence of trusted ownership. The installed service's capability set lacks `CAP_CHOWN`, so do not assume the daemon itself can successfully take ownership of a foreign-owned state root: that operation may fail. The residual daemon scenario is a root-owned state root populated with untrusted files/children, for example after privileged staging; direct mode alone already demonstrates the blocker.

These attacks require unsafe/custom/preseeded state or a bad ancestor. Nothing in this trace gives a normal unprivileged user access to an already-correct default `/var/lib/tidemark` beneath trusted root-owned parents. The parent audit separately reproduced the direct SQLite symlink sink in its own temporary fixture; no privileged exploit or deployment operation was run.

### Checks performed today

| Boundary | Checks present | Missing trust or race protection |
|---|---|---|
| CLI state selection | `tidemark-cli/src/lib.rs:111–123` applies `--state-dir`, validates config; core checks absolute path at `config.rs:748`. | No state filesystem ownership/mode/symlink inspection before direct preflight. |
| Direct scan admission/lock | `cmd/scan.rs:156–168` requires absent/refused daemon, acquires lock, rechecks daemon, opens catalog; `orchestrator.rs:684–699` mkdir 0700, lock open 0600 + NOFOLLOW, nonblocking exclusive flock. | Existing state permissions/owner and ancestors not checked. Leaf lock type/owner/link count not checked. All state calls use original pathname. |
| Direct/catalog database | `catalog.rs:119–130` mkdir when writable; `timeseries.rs:33–93` header preflight, exclusive 0600 creation if absent, supported schema/version, in-memory scratch, busy timeout and WAL. | Preflight opens with ordinary `File::open` (`:97`), can follow/block; separate SQLite pathname reopen (`:50`). Existing main/sidecars have no explicit type/owner/mode/link validation at the store boundary. |
| Snapshot writer | `writer.rs:32–61` directories requested 0700; exclusive temporary create requested 0600; existing final ID refused; temporary fd chmod; publication `RENAME_NOREPLACE` at 344–350, cancellation checkpoints. | Existing directories are not checked; temp fd is closed then SQLite reopens name, sync reopens name (`:341`), final rename uses path, drop removes path (`:366`). Unsafe ancestors or writable snapshot children can replace names between calls. |
| Snapshot reader | `reader.rs:159–179,293` URI percent-encodes raw bytes, `immutable=1&mode=ro`, read-only SQLite flags and in-memory scratch; schema validation. | No no-follow leaf/ancestor, file owner or regular/single-link check. Read-only prevents intentional writes, not disclosure/reads through an attacker-selected referent or parser exposure to mutable state. |
| Corrupt catalog recovery | `catalog.rs:339–373` creates unique private recovery directory, renames original main/WAL/SHM rather than destroying contents; called only after classified corruption. | Recovery path operations rely on unproven state provenance in direct mode. No-follow cleanup elsewhere does not repair this. |
| Crash temporary cleanup | `catalog.rs:416–478` no-follow directory opens, O_PATH binding, same mount ID checks (including bind mounts), regular temporary entries only, descriptor-relative unlink; bounded streaming. | Initial state path's ancestors can still follow in direct mode; no root directory ownership proof. Within a correctly trusted retained state fd, its cleanup boundary is substantially stronger than the writer's generic path API. |
| Scan retention | `orchestrator.rs:1017–1028` canonicalizes allowed snapshots and target parent, checks containment and `.db`, then removes pathname. | Canonicalize-to-unlink is a race if state ancestry/children are attacker-writable. Descriptor-relative deletion beneath a verified root avoids that dependence. |
| Daemon state preflight | `lib.rs:1207–1269` rejects unsafe protected/root paths, symlink/nondirectory state/deploy/snapshots/tmp ancestry, state/deploy mount roots, malformed/path-mismatched marker and deployment artifacts; checks unmarked state and deploy owner against euid/no group/world bits. | Owner/write checks absent for all ancestor components; marker bypass of state owner/mode; snapshots/tmp and lower directories lack owner/mode checks. |
| Daemon persistent files | `lib.rs:1279–1299` checks main/WAL/SHM/bak-v0 regular or absent; `state.rs:421–432` retains state fd for SQLite path anchoring. | Existing files' owner/mode/link count not checked; journal not included; snapshot descendant trust incomplete. A check at startup is sufficient against ordinary users only after their ability to alter the directory/files is excluded. |
| Deployment source/config reads | `deploy/files.rs:178–213` component-relative no-follow traversal, final NOFOLLOW/NONBLOCK, regular type, 1 MiB cap. Stage parses and hashes captured bytes. | Source is an administrator-selected input; this is not a proof of trustworthy file ownership. Installed destination ancestry is similarly no-follow but not protected against replacement by another directory owner. |
| Stage state operations | `deploy/mod.rs:300–317` all blocking checks before service changes; handles retained at 317/374; state/record writes and compensation use those handles at 386–408. `files.rs:323–341` exclusive 0600 temporary, fsync, descriptor-relative rename and dir sync. | Marker provenance and ancestor trust as above. Installed config/backup operations reopen their pathname parent separately (`env.atomic_write` at 378/381, `files.rs:294`), so untrusted ancestor changes can move their destination. A final-leaf symlink never becomes the write target because publication is rename-over-entry. |
| Ordinary unstage | `deploy/mod.rs:668–680` validates state, retains handles, reads marker-referenced captured config and checks roots before `disable_now`; then checks stopped and unlinks staged record through deploy fd (`:693`). | State/marker trust gap remains, but ordinary unstage does not recursively delete state or follow a staged-record leaf. |
| Purge | `deploy/purge.rs:83–126` no-follow parent/root, regular bounded marker, exact marker/config state match, cfg validity, root/parent mount IDs; preflight iterative tree walk. Removal at 133–205 streams width, caps depth 256, no-follow child directories, checks mount IDs and dev/ino before removing directory entries/root; symlinks are unlinked, not followed. | Marker's “authentic” wording is too strong: contents are not authenticated by trusted ownership. Parent ownership/replacement trust still missing. Descriptor retention/identity checks are good defenses, but a mutable untrusted tree can be moved around while traversal is in progress. |

### Smallest correct repair recommendation

Put one **verified state-directory opener in the storage layer**, called before direct-mode `ScanLock` or any catalog/series/snapshot/recovery mutation, and reuse its directory policy for daemon/deploy. Keep the original user-visible configured path for reports and exclusion semantics; use a retained verified fd for operations. There is no need for a new CLI flag or root uid startup gate.

1. Walk from `/` through component-relative `O_DIRECTORY|O_NOFOLLOW` opens (NOATIME with EPERM fallback), and validate ownership and effective write authority at every ancestor before trusting a child. For root production, reject user-owned or group/world-writable replaceable ancestry. Preserve legitimate test/private `/tmp` layouts only with an explicit sticky-directory rule: a trusted-owned sticky parent may be acceptable when the actual child being traversed is trusted-owned and cannot be renamed by another user; a user-owned intermediate directory still fails. Retain the actual descriptor used for the checks. Default ACL inheritance must not silently create untrusted children; validate the resulting fstat mode/ownership after creation.
2. Require the state root and all used state subdirectories to be euid-owned/private (0700 in production), or create them safely relative to verified parents. A marker may authorize normalizing a **trusted-owner** root's mode only after the marker itself is proven regular, trusted-owner/private and single-link; marker text must never waive foreign ownership. Reject unsafe preexisting files rather than chmod/chowning arbitrary targets into apparent trust.
3. Before header reads/SQLite opens, validate existing main, WAL, SHM, rollback journal, backup, lock and snapshot files through the verified directory: no symlink, regular file, trusted owner, private mode and single link. Open header/lock files with NOFOLLOW/NONBLOCK and verify the opened fd. Use SQLite's NOFOLLOW option for main opens where available; do not claim it secures sidecars. Trusted nonmutable directory/file provenance is what removes the check/reopen race for SQLite's path VFS and sidecars. Keep publication, recovery and deletion relative to the retained directory; give direct mode the same lifetime binding the daemon already has.
4. Exercise unmarked foreign state, forged markers, a private state root under foreign-owned replaceable ancestry, foreign-owned/hard-linked database leaves, symlink/FIFO leaves, unsafe snapshot descendants, and deterministic rename-at-checkpoint replacements. Assert refusal before any outside data/mode change or service operation. Keep positive euid-owned temporary-directory tests and the retained-daemon-fd ancestor-rename case to avoid weakening the existing anchoring fix.

An exact follow-up file set for that repair is: new `crates/tidemark-store/src/state_dir.rs`; module declaration in `crates/tidemark-store/src/lib.rs`; storage callers `catalog.rs`, `timeseries.rs`, `writer.rs`, `reader.rs` and `space_guard.rs` if it uses the shared anchor; `crates/tidemark-scan/src/orchestrator.rs`; direct callers `crates/tidemark-cli/src/cmd/scan.rs`; deployment `crates/tidemark-cli/src/deploy/{files.rs,mod.rs,purge.rs}`; daemon `crates/tidemarkd/src/lib.rs`; focused tests in the new state helper and existing `crates/tidemark-cli/src/deploy/tests.rs`, `crates/tidemarkd/tests/w6a1.rs`, and scan/store regression targets. Reuse `LiveState`'s existing bound path; changing `state.rs` or `scheduler.rs` is not required merely to fix its already-correct descriptor anchoring. Shared public helper additions should follow the project's normal contract review rather than silently changing frozen interfaces. This is a recommendation, not a change performed by the audit.

### Old F02 no longer holds

Stage now captures one bounded no-follow regular-file read at `deploy/mod.rs:151–161`, validates `Config::from_toml_bytes(&bytes)`, carries `(cfg, bytes)` from `env.source` at 300, hashes those same bytes at 322, and publishes them at 381. Purge and marker-root validation likewise parse the captured bytes (`purge.rs:110–111`, `files.rs:165–166`). Daemon `load_config_captured` uses a bounded no-follow/NONBLOCK read, then `parse_config_bytes` parses/hashes the same slice (`tidemarkd/src/lib.rs:1334–1372`). The source replacement race from the old report is fixed. Tests named `source_replacement_does_not_change_parsed_or_installed_bytes` and `stage_installs_and_hashes_the_config_parsed_before_replacement` are present at `deploy/tests.rs:859,1012`; this audit inspected but did not execute them. Destination/ownership issues described in F01 remain separate.

### Deployment behavior and review limits

Stage's only skippable checks are the fanotify/bulkstat/LVM warnings (`deploy/mod.rs:291–298`); install/config/roots/local-state/mount/space failures block before service mutation (`:270–277,311–317`). It allows only the local filesystem names at `deploy/real.rs:206–216`; the locality check is stage-only, leaving unstage available for removal. Its service target is the fixed unit string, using `/usr/bin/systemctl` with argument arrays, not shell interpolation (`deploy/mod.rs:733–750`). Post-preflight errors attempt config/record/backup and enabled/active-state compensation and report individual failures (`:394–449`); this is not transactional and can leave newly created empty directories or changed times. No new finding is needed merely for that documented behavior.

## Privilege, network and input boundaries

The deployed unit at `packaging/systemd/tidemarkd.service:13–35` runs root, sets UMask 0077, a 2 GiB cgroup memory limit, CPUQuota 200%, NOFILE 65,536, private state/runtime directories, NoNewPrivileges, and four bounded capabilities. Namespace restrictions, native syscall architecture and personality/realtime restrictions apply. The absence of filesystem namespace sandboxing is intentional ADR-013 behavior: a separate mount namespace can make mount marks observe the wrong mount objects. These are useful restrictions, **not a kernel-enforced read-only filesystem**. Root ownership and these capabilities still give a compromised daemon considerable power; ADR-008's aspirational “a compromise can't corrupt data” wording is not an established security property.

| Capability | Current use and whether it can be removed |
|---|---|
| `CAP_SYS_ADMIN` | XFS bulkstat (`tidemark-sys/src/xfs.rs:200,214`) and fanotify initialization/marks (`fanotify.rs:88,129`). Removing it from the current EL8 deployment removes required privileged observation. A future narrowly scoped metadata helper could reduce the main daemon's capabilities, but that is an architectural change, not a tested drop-in hardening setting. |
| `CAP_DAC_READ_SEARCH` | Traversing directories belonging to other users through walker no-follow directory opens; FID `open_by_handle_at` also depends on this privilege. Even though measured EL8 mode is fd mode, dropping it loses complete protected-tree coverage. It does not grant general write permission. |
| `CAP_FOWNER` | `O_NOATIME` on foreign-owned directories (`tidemark-fs/src/walk/getdents.rs:63–68,104–105`). Dropping it permits the coded EPERM fallback, which can cause directory atime writes. UID 0 alone is insufficient; this capability is needed to preserve the intended no-atime production behavior. It also makes ownership-sensitive operations more powerful after compromise. |
| `CAP_SYS_PTRACE` | Reading/statting other processes' procfs fd and identity metadata (`tidemark-fs/src/procfs.rs:190–258`, attribution helpers). No attach/ptrace call is used. Dropping it can degrade attribution/open-deleted-file visibility under kernel procfs permission checks. A separately privileged process collector is a future narrowing option. |

No production first-party TCP/UDP/Internet socket constructor or HTTP client/server remains in the searched workspace. `Cargo.lock` has no HTTP/TLS stack packages (including http, httparse, tiny_http, ureq, reqwest, hyper, rustls, openssl, native-tls, ring or webpki variants). The unit restricts address families to exactly `AF_UNIX AF_NETLINK`; there is no hidden HTTP flag. ADR-020 overrides older diagram/ADR-006/webhook wording. LVM collection runs plain `lvs`/`vgs` reporting arguments (`tidemark-fs/src/lvm.rs:225–240`) with no `--readonly`, no shell and no mutation command.

“Only the Unix API socket” needs a precise interpretation: the sole application **listener** is `UnixListener::bind` in `tidemarkd/src/api.rs:703`. The daemon also creates outbound Unix datagram sockets through `tracing_journald::layer()` (`lib.rs:116`) and `sd_notify::notify` (`:882,:948`); their cached implementations use `UnixDatagram::unbound`. API stale-listener probes also use AF_UNIX. Netlink remains allowed for reporting tools/udev. No network trace or seccomp runtime test was executed, so this establishes source/configuration intent rather than a universal statement about every OS library or administrator-supplied helper binary.

| Locally influenced input | Neutralization/bounds and remaining limit |
|---|---|
| File/name bytes, width and depth | `RawName` rejects slash/NUL/dot/dot-dot; `RawPath` is byte-preserving and lexical (`tidemark-core/src/path.rs:20,63`). Traversal is iterative. Walk work/record caps, bulk estimates, aggregate limits and error truncation are explicit; see the availability section. A resource limit causes failure/degradation rather than a falsely complete truncated directory. These caps are not a whole-process byte reservation. |
| Symlink entries and replacement races | Walker opens every component with NOFOLLOW/DIRECTORY, stats with NOFOLLOW/NO_AUTOMOUNT and checks expected directory inode/device/available mount ID (`walk/getdents.rs:74–140`, `worker.rs:324–395,516`). Bulk re-reads names through the checked helper (`bulk/names.rs:79`). Replacing a queued name with a symlink fails before payload access. Symlink analysis intentionally reads link text and optionally stats a target; its atime exception is F04. |
| Hard links | Identity is `(dev,ino)`, not a pathname assumption; link accounting and alias resolution remain metadata-only. A user can change links concurrently, so observed counts/names are a nontransactional view. No new hard-link payload read was found. This review did not reprove all accounting equations or du equivalence. Hard links inside a preseeded state tree are a different, unsafe persistent-file boundary (F01). |
| Mount changes and bind mounts | Startup mountinfo produces skip paths; available statx mount IDs and device checks reject changed identities. On the measured EL8 kernels statx lacks mount IDs: a bind introduced after the mountinfo snapshot that preserves device/inode cannot always be distinguished in walker/checked-name paths (`getdents.rs:251–259`); R2-2 explicitly records this limit. Ordinary users cannot alter the daemon's host mount namespace with their own private user namespace. Host mount-management authority remains trusted. |
| Configured mount disappearance/remount | Current code is newer than the plan's open-risk list: sampler compares retained fd mount ID from bounded fdinfo before statvfs (`services/sampler.rs:32–61`), and watcher service checks actual `w.mount_identity()` after starting (`services/watcher.rs:107–117`). State clears current samples/rings on identity transition (`state.rs` presence/publication paths). This avoids substituting the underlying directory and joining rates across remounts; no new guest remount trial was run. |
| Process names/cgroups/fd paths | Numeric procfs paths, byte/text caps, descriptor `(dev,ino)` recheck and starttime before/after the pass (`procfs.rs:196–258`) prevent names from becoming opened data paths and reduce PID/fd reuse misattribution. Attribution cache has bounded entries and watcher-only subsecond reuse. Processes may exit/change comm/cgroup at any moment; labels are best-effort, not security identities. Large total retained fd-path bytes remain F03. |
| API socket directory | Current API opens/checks every ancestor and retains its parent (`api.rs:439–529`), rejects untrusted owner/nonsticky writable ancestry, requires a private owned child below sticky ancestors, validates/flocks an owned single-link lock, and binds relative to the retained parent. Final socket ownership/mode/identity checks protect stale removal/chmod/cleanup. This is materially stronger than the state and metrics helpers. Default `/run/tidemark` 0700/socket 0600 blocks ordinary users; uid-0 SO_PEERCRED authorization occurs before input parsing even if DAC permits connection. |
| State ancestors and leaves | Direct mode is still unsafe; daemon anchoring and nonregular-file checks are real improvements but do not establish ownership of preseeded descendants or authenticate marker text. Full current trace and minimum fix are in the state section (F01). |
| Metrics collector directory | Staging fd is no-follow, regular/euid-owned/single-link, flocked, identity-checked before truncation, synced and renamed; old valid temporary files can be reused. Parent provenance is unchecked before its ordinary open, and directory-entry mutation is possible if the collector is user-writable (F02). |
| `deploy stage`/`unstage` inputs | Bounded captured source bytes are parsed/installed/hashed consistently. Unsafe state root/mount/containment checks precede service mutation; the source/destination helpers reject symlinks through component opens. Service executable/unit are fixed and arguments are passed without a shell. Post-preflight compensation is best effort, not transactional. A marker does not confer trustworthy ownership; unsafe ancestry and descendants remain F01. Purge additionally requires explicit `--purge-state --yes` guards, validates marker/config state equality, walks iteratively and refuses crossed mounts. It is intentionally destructive only to accepted Tidemark state; no purge was run here. |

### Output escaping

Normal scanner/live/TUI display paths use `RawPath::display_lossy` or `display_lossy_bytes`, which escapes C0/C1 control characters, backslashes and invalid UTF-8 (`path.rs:253–282`); table shortening preserves escaped byte tokens. Hot/process comm/unit values likewise pass through that helper (`render/hot.rs:43,47`, `cmd/watch.rs:212–222`, `render/now.rs:98–101`). Printable Unicode is retained, including visually confusable or bidirectional formatting text; this is not a visual-spoof-proof presentation policy.

JSON is serialized by serde_json; valid UTF-8 paths retain original text with JSON escapes, invalid bytes use the documented base64 sibling. JSON-safe encoding is not a license to interpolate decoded values into a terminal or shell. Prometheus labels escape backslash, quote and LF (`metrics.rs:211–214`), and intentionally replace invalid UTF-8. They preserve other valid control/Unicode characters, including ESC: the textfile is a machine format and is **not terminal-safe if printed raw**. Metric values never become format strings, and escaped LF/quote prevent adding metric lines through a label.

Alert subjects from paths, comm/unit hot keys and LVM names are escaped in `alerts/rules.rs:117,141,153,170,181,190,211,217`; journal delivery uses structured fields (`alerts/deliver.rs:20`). The journald dependency uses length-encoded fields, so embedded newlines do not create new journal fields. Rule names are trusted configuration text, and generic `%error` logging can contain raw diagnostic text; the fallback tracing formatter is not an output-sanitization boundary. It would be false to say **all** human/log output is escaped: deployment eligibility formats `Path::display` at `deploy/files.rs:401`, reaching raw check-detail output and CLI errors (F06). Probe fact/advice strings also rely on their trusted host-metadata producers rather than a universal escaping layer (`render/probe.rs:62,79,82`). No ordinary scanned-entry ANSI injection was found in the reviewed normal result renderers.

## API boundary: confirmed controls and residual limits

| Surface | Current evidence | Conclusion |
|---|---|---|
| Socket ancestors | `api.rs:438-496`: absolute path, component-by-component O_DIRECTORY\|O_NOFOLLOW, trusted ownership, nonshared ancestors; sticky ancestors require an owned 0700 immediate child; final parent trusted/nonshared. | Ordinary users cannot substitute a configured accepted writable ancestry. Read-only untrusted-owner mounts are explicitly accepted; hostile filesystem/kernel behavior was not tested. |
| Socket and lock leaf | `api.rs:498-528`: owned single-link regular no-follow lock; `:533`: retained-parent procfd address; `:537-658`: no-follow stat, inode identity checks, pinned O_PATH socket chmod0600 and identity-aware cleanup. | Local unprivileged pathname replacement is excluded by trusted parents. No promise against another root process is needed. |
| Peer identity | `api.rs:674-677`: kernel SO_PEERCRED; `:302-307`: nonzero uid and lookup failure close before request allocation/parsing; `:156`: dispatch also checks uid. | Packaged `/run/tidemark` 0700 and socket 0600 deny ordinary connect; even a changed permission does not grant method access to nonroot peers. No unprivileged slow-client route verified. |
| Connection and handler retention | `api.rs:14-16`, `:714-774`: at most 32 retained connection workers/admissions; `:217-225`, `:375-395`: owned handler join guard and retained permit. | No detached handler growth after timeouts. Per admitted connection there can be one connection thread plus one handler thread. |
| Request/response framing | `api.rs:309-367`, `:423`: absolute 10-second first/incomplete frame budgets with 50 ms read polling; `:340`:1 MiB request; `:234-266`:1 MiB serialized response; `:267-289`: separate absolute 10-second write budget. | Drip bytes do not extend an incomplete frame. Root clients can keep making complete requests, within 32 admissions. The response serialization cap is applied after result construction/serde_json::to_value (`:113`), so it is not a total-memory cap for large measurement objects. |
| Accept pressure | `api.rs:801-845`: EMFILE/ENFILE/ENOMEM/ENOBUFS via resource_pressure plus ECONNABORTED retry;50-800 ms backoff, once-per-minute warning; server waits in 20 ms shutdown-aware slices (`:740`). | The former event-flood-to-EMFILE-to-global-shutdown path is addressed for these recoverable errors. Other unexpected accept errors still set shutdown (`:785-788`). |
| Handler cancellation | `api.rs:397-421`: deadline signals cancellation; guard joins handler on exit. `lib.rs:1007-1027` joins even after shared shutdown deadline is missed. | In-flight kernel/SQLite operations can still delay response completion or shutdown. This is bounded handle ownership, not interruptible I/O or a proven hard 10-second lifecycle guarantee. Root-only API limits attacker access but local filesystem/procfs work may cause long service work. |

## Availability evidence by input

| Input | Actual bounds/recovery | Remaining exposure |
|---|---|---|
| Fd-mode event floods | `watch/mod.rs:22-36`:16-256 fixed24-byte records/read,6144-byte maximum transport buffer (`:361`); successful records close through RAII; descriptor inventory is bounded (`:46-90`). `tidemarkd/lib.rs:458-497` raises soft to hard, computes `(soft-512)/mounts`, clamps16-256 and warns if headroom is unsatisfiable. `services/watcher.rs:59-68` retires old watchers before rebudgeting. | Satisfiable configured budgets prevent the previous256-per-mount batch burst from consuming the full daemon fd table. If soft is below 512+16*mounts, reserve cannot be guaranteed. Other daemon consumers/kernel pressure still compete; no guarantee of no loss or latency. |
| Overflow/resource recovery | `watch/mod.rs:659-747`: retryable transport failures clear path/handle caches, mark loss, bounded backoff 50-800 ms; queue overflows counted separately (`:756-760`). `services/watcher.rs:73-89`, `:143-145` retain state under transient mount/namespace errors and report degraded. | An unprivileged writer can deliberately cause lossy measurement and sustained processing load. Nonrecoverable transport errors put watcher in Failed; this service reports failure rather than restarting an existing failed Arc. No attacker-controlled nonrecoverable transport errno was established. |
| Watcher state | `watch/rings.rs:6-10`, `:94-138`, `:177-193`:20k current keys/scope,1k completed keys/scope,60 buckets,200k current unique tuples; per-row8 unit counters. `watch/mod.rs:196`, `:547`, `:872`:1024 loss acknowledgements, optional1000 foreground events (disabled in daemon),200k dirty paths. Size history uses configured LRU; handle paths capped100k (`:374-376` and fdmode/fidmode implementations). | These are cardinality bounds, not a unified byte/RSS budget. Keys and retained dirty paths are attacker influenced; completed copies plus simultaneous scan/SQLite memory can consume substantial 2 GiB headroom. No unbounded watcher collection was established. |
| Deep/wide directory trees | `walk/worker.rs:23-27`:65536 pending tasks,64 MiB pending path bytes,1million items/directory,10000 retained errors; record bytes checked at `:850`; task admission`:543-563`; threads1-64/output1-1024/buffer4 KiB-4 MiB at `:70-79`. `walk/getdents.rs:86-138` caps a path at 1 MiB and opens components iteratively. | No recursion proportional to directory depth in these walkers. Excess width/path/record memory returns a scan error, so a user can prevent successful coverage of their pathological subtree but need not kill daemon. Repeated long-path reopening can still be expensive; cancellation/gating bounds work progression, not malicious workload impact. |
| Bulk scan | `scan/bulk/mod.rs:65-98`, `:124`, `:398-415`:saturating memory estimate, validated batch/thread memory, queue2,32 MiB worker allowance; `bulk/join.rs:15`,`:50-60`:256 MiB dent cap and fallible reservation; same namespace walker used. | Limits cause explicit refusal rather than unlimited join growth. No privileged kernel-bulk or total RSS validation performed. |
| Process churn | `fs/procfs.rs:12-14`,`:43-50`,`:53-57`,`:195-257`:bounded numeric PID/fd inventory,1 MiB text fields, transient vanished/inaccessible processes skipped, PID starttime and descriptor inode/dev rechecks. `live/procscan.rs:20-22`,`:266-302`:10k retained writers/60 samples. | Initial full inventory/path allocations are F03. Processes may exit faster than scan, giving best-effort omission; the pass has no cancellation token and can run long. `services/procscan.rs:65-105` only checks shutdown between passes. |
| Published process results | `procscan.rs:344-352`:immutable completed report cloned outside publication; `state.rs:553-558`:Arc swap; `services/procscan.rs:89-100`:ordinary failures publish unavailable, resource failures retain previous report and degrade. | Readers no longer block on collection, but cloning large phantom results/JSON is still substantial work/memory. Retained stale data under resource failure must be interpreted together with component degradation. |
| Persistence pressure | `services/tswriter.rs:60-63`:64 messages/20000 rows each; procscan producers try_send (`services/procscan.rs:93-94`) and report drops. | Queue count is bounded. Payload bytes depend on attacker-generated path lengths/counts; count bounds alone do not establish a2 GiB budget. No allocator abort recovery claim. |

## Direct answers for part 6

- **Can an ordinary local user force daemon exit?** Yes, on hosts permitting the descriptor inventory described in F03, the process scan's count-only cap permits live pathname allocations alone above the packaged 2 GiB cgroup limit. OOM/restart is inferred from the verified allocation bound; not executed here. Fd-batch resource errnos themselves now degrade/retry rather than taking the old fatal API path.
- **Can they stop answering?** They cannot normally connect as slow API clients, because of root-only directory/socket permissions and SO_PEERCRED. They can make watched statistics lossy and consume scan/process-worker resources; OOM kills the entire daemon. In-flight procfs/filesystem/SQLite calls and uncancellable process passes have no hard latency proof. No separate unprivileged API deadlock was verified.
- **Can they grow a checked structure without bound?** The examined inventories/queues have finite count caps, so literal unbounded retained collection growth was not established. Those caps allow excessive byte consumption before refusal and do not provide the required service RSS guarantee.

## Dependency review of the current tree after HTTP removal

HEAD reviewed: `fb8021cb14b1c31c2c6a6d46a7b3822bcf191cbb`. Commands were offline, locked and read-only; cached source paths came from Cargo metadata. No source build, advisory refresh, dependency update or network access was performed. This is a package/source inventory and data-flow classification, not an independent audit of every dependency unsafe block, nor proof of which generic functions survive linking.

Artifacts: `metadata.json` (Cargo metadata), `linux-runtime-tree.txt` (normal edges, Linux x86_64, proc-macro subgraphs omitted), and `dependency-inventory.json` (package source paths, unsafe token files and representative syntactic matches). Commands:

```sh
CARGO_HOME=/tmp/tidemark-audit2-8ccb-cargo RUSTUP_HOME=/home/brewerm/.rustup PATH=/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH cargo metadata --offline --locked --format-version 1
CARGO_HOME=/tmp/tidemark-audit2-8ccb-cargo RUSTUP_HOME=/home/brewerm/.rustup PATH=/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH cargo tree --offline --locked --target x86_64-unknown-linux-gnu --edges normal,no-proc-macro -p tidemark-cli -p tidemarkd --prefix none --no-dedupe
```

Cargo metadata resolves **294 packages (284 external + 10 workspace)** across workspace/dev/build/targets. The selected CLI+daemon Linux normal closure has **129 package-version identities (121 external + 8 workspace)**. These are graph counts, not binary linkage counts. Of the 121 external packages, 87 have an `unsafe` token in `src/**/*.rs`, 82 have syntactic unsafe block/function/impl/trait definitions after ignoring comment-only matches, and five have only prose/lints. Ten of those 82 require exclusions/qualification below. The remaining **72 package-version identities** have applicable or potentially instantiated unsafe library/macro code; they are all named in the inventory. Some handle configuration or lifecycle rather than scanned metadata directly. Workspace-wide metadata feature lists can include dev/build unification; where an exclusion matters it was checked against selected code/target and feature conditions.

### Unsafe-bearing packages in the normal Linux closure

Each row names every package in that purpose group. The cached-source examples table below provides one location per package. The relationship is package-level: a row does not assert every unsafe function in the package receives attacker-controlled scan data.

| Role | Packages (locked versions) | Data / justification |
|---|---|---|
| OS and storage boundary | `libc 0.2.190`, `libsqlite3-sys 0.36.0`, `linux-raw-sys 0.12.1`, `rusqlite 0.38.0`, `rustix 1.1.5` | Native syscall/ABI wrappers and SQLite FFI act on descriptor metadata, raw path bytes and persisted scan rows. Bundled SQLite C is a native dependency even though C has no Rust unsafe keyword. |
| Metadata allocation and containers | `allocator-api2 0.2.21`, `hashbrown 0.16.1`, `hashbrown 0.17.1`, `hashlink 0.11.1`, `smallvec 1.16.3` | Allocation/raw-table or inline storage holds metadata keys, links, records or cached SQL statements; hashbrown has two locked versions. |
| Concurrent scan pipeline | `crossbeam-channel 0.5.17`, `crossbeam-deque 0.8.8`, `crossbeam-epoch 0.9.21`, `crossbeam-utils 0.8.23`, `rayon 1.12.0`, `rayon-core 1.13.0` | Channels/work stealing/epoch reclamation and parallel sorting move and retain scan records. Unsafe memory ownership and synchronization are below first-party safe interfaces. |
| Shared-state synchronization | `lock_api 0.4.14`, `parking_lot 0.12.5`, `parking_lot_core 0.9.12`, `scopeguard 1.2.0` | Mutex/rwlock and guard implementation protects watcher, scan, API and collector state; unsafe cell access, locking invariants and cleanup primitives affect retained data. |
| Name matching and hashing | `aho-corasick 1.1.5`, `bstr 1.13.1`, `foldhash 0.2.0`, `memchr 2.8.3`, `regex-automata 0.4.18`, `xxhash-rust 0.8.19` | Byte/UTF-8 search, automata and hashing consume raw names, paths or parsed metadata. SIMD, unchecked indexing and raw byte loads make bounds/encoding invariants part of the trust boundary. |
| Serialization and number output | `itoa 1.0.18`, `ryu 1.0.23`, `serde_core 1.0.229`, `serde_json 1.0.152`, `zmij 1.0.23` | Metadata/paths and API/config/state fields are serialized or parsed. Unsafe UTF-8 construction, numeric conversion buffers and digit generation sit beneath public safe interfaces. |
| Display buffers and iterators | `castaway 0.2.4`, `compact_str 0.9.1`, `either 1.19.0`, `itertools 0.14.0`, `lru 0.18.5`, `num-traits 0.2.19` | TUI/report display and indirect iterator/container support handle derived metadata, geometry or formatted values. Their packages contain unchecked storage/transmutation/numeric or Pin helpers; package reachability is not proof each unsafe method is instantiated. |
| Terminal/CLI processing | `anstream 1.0.0`, `anstyle 1.0.14`, `anstyle-parse 1.0.0`, `clap_lex 1.1.1`, `crossterm 0.29.0`, `utf8parse 0.2.2` | CLI output/ANSI parsing, UTF-8 reconstruction, terminal ioctl and byte-oriented option handling. Some process only terminal state/options rather than scan names; terminal/report code shares their process and output pipeline. |
| Logging and registry | `lazy_static 1.5.1`, `log 0.4.34`, `matchers 0.2.0`, `once_cell 1.21.4`, `sharded-slab 0.1.7`, `thread_local 1.1.10`, `tracing 0.1.44`, `tracing-core 0.1.36`, `tracing-journald 0.3.2`, `tracing-log 0.2.0`, `tracing-subscriber 0.3.23` | Metadata-bearing diagnostics pass through logging fields, formatting/filtering, registries and thread-local caches. Unsafe lifetimes, erased pointers, initialization and journal socket ancillary-data code underlie safe APIs. |
| Timestamp formatting | `chrono 0.4.45`, `deranged 0.5.8`, `powerfmt 0.2.1`, `time 0.3.55` | Scan timestamps, scheduler clocks and rendered values use date/time types and formatting; unsafe invariants/range wrappers and local-time FFI exist in these packages. This does not mean arbitrary pathname bytes enter every unsafe function. |
| Error transport | `anyhow 1.0.104` | Erased error ownership/downcast implementation transports errors that can contain scanned paths. |
| Configuration parsing/hash support | `block-buffer 0.10.4`, `cpufeatures 0.2.17`, `generic-array 0.14.7`, `sha2 0.10.9`, `toml_parser 1.1.5+spec-1.1.0`, `winnow 1.0.4` | TOML source and parser slices plus config digest buffers. These process operator config, including configured roots/exclusions; cpufeatures chooses CPU routines, not scanned file payload. No cryptographic network/TLS role remains. |
| Process and terminal lifecycle support | `errno 0.3.14`, `getrandom 0.4.3`, `mio 1.2.4`, `pin-project-lite 0.2.17`, `sd-notify 0.4.5`, `signal-hook 0.3.18`, `signal-hook-registry 1.4.8` | Errno FFI, entropy for temporary state names, terminal poll/notification, systemd status and signal shutdown infrastructure. These packages contain unsafe but are not direct scanned-payload consumers. pin-project-lite defines unsafe projection macros; no asynchronous runtime is used by Tidemark. |

### Source evidence for every listed package

Paths here are relative to each cached package root identified in `dependency-inventory.json`. Representative locations establish code presence; they are not site-by-site safety verdicts. Examples were selected to avoid disabled features and foreign-platform implementations where available (Linux epoll for mio, Linux raw syscall path for rustix, xxh3 for the configured xxhash feature, standard float output for ryu). Platform/feature gates remain material; presence does not prove execution of an individual generic helper.

| Package | Representative unsafe source | Role |
|---|---|---|
| `aho-corasick 1.1.5` | `src/automaton.rs:198` | Name matching and hashing |
| `allocator-api2 0.2.21` | `src/stable/vec/splice.rs:54` | Metadata allocation and containers |
| `anstream 1.0.0` | `src/adapter/strip.rs:136` | Terminal/CLI processing |
| `anstyle 1.0.14` | `src/color.rs:615` | Terminal/CLI processing |
| `anstyle-parse 1.0.0` | `src/state/definitions.rs:116` | Terminal/CLI processing |
| `anyhow 1.0.104` | `src/ptr.rs:13` | Error transport |
| `block-buffer 0.10.4` | `src/sealed.rs:27` | Configuration parsing/hash support |
| `bstr 1.13.1` | `src/utf8.rs:341` | Name matching and hashing |
| `castaway 0.2.4` | `src/utils.rs:53` | Display buffers and iterators |
| `chrono 0.4.45` | `src/naive/date/mod.rs:1496` | Timestamp formatting |
| `clap_lex 1.1.1` | `src/lib.rs:458` | Terminal/CLI processing |
| `compact_str 0.9.1` | `src/traits.rs:56` | Display buffers and iterators |
| `cpufeatures 0.2.17` | `src/x86.rs:47` | Configuration parsing/hash support |
| `crossbeam-channel 0.5.17` | `src/select_macro.rs:766` | Concurrent scan pipeline |
| `crossbeam-deque 0.8.8` | `src/deque.rs:56` | Concurrent scan pipeline |
| `crossbeam-epoch 0.9.21` | `src/sync/queue.rs:40` | Concurrent scan pipeline |
| `crossbeam-utils 0.8.23` | `src/thread.rs:224` | Concurrent scan pipeline |
| `crossterm 0.29.0` | `src/tty.rs:33` | Terminal/CLI processing |
| `deranged 0.5.8` | `src/unsafe_wrapper.rs:18` | Timestamp formatting |
| `either 1.19.0` | `src/lib.rs:438` | Display buffers and iterators |
| `errno 0.3.14` | `src/unix.rs:53` | Process and terminal lifecycle support |
| `foldhash 0.2.0` | `src/seed.rs:193` | Name matching and hashing |
| `generic-array 0.14.7` | `src/sequence.rs:12` | Configuration parsing/hash support |
| `getrandom 0.4.3` | `src/backends/linux_android_with_fallback.rs:82` | Process and terminal lifecycle support |
| `hashbrown 0.16.1` | `src/util.rs:37` | Metadata allocation and containers |
| `hashbrown 0.17.1` | `src/scopeguard.rs:34` | Metadata allocation and containers |
| `hashlink 0.11.1` | `src/linked_hash_map.rs:110` | Metadata allocation and containers |
| `itertools 0.14.0` | `src/either_or_both.rs:393` | Display buffers and iterators |
| `itoa 1.0.18` | `src/lib.rs:108` | Serialization and number output |
| `lazy_static 1.5.1` | `src/inline_lazy.rs:36` | Logging and registry |
| `libc 0.2.190` | `src/types.rs:123` | OS and storage boundary |
| `libsqlite3-sys 0.36.0` | `src/lib.rs:20` | OS and storage boundary |
| `linux-raw-sys 0.12.1` | `src/x86_64/system.rs:120` | OS and storage boundary |
| `lock_api 0.4.14` | `src/rwlock.rs:38` | Shared-state synchronization |
| `log 0.4.34` | `src/lib.rs:1438` | Logging and registry |
| `lru 0.18.5` | `src/lib.rs:95` | Display buffers and iterators |
| `matchers 0.2.0` | `src/lib.rs:241` | Logging and registry |
| `memchr 2.8.3` | `src/vector.rs:33` | Name matching and hashing |
| `mio 1.2.4` | `src/sys/unix/selector/epoll.rs:29` | Process and terminal lifecycle support |
| `num-traits 0.2.19` | `src/cast.rs:284` | Display buffers and iterators |
| `once_cell 1.21.4` | `src/race.rs:68` | Logging and registry |
| `parking_lot 0.12.5` | `src/util.rs:12` | Shared-state synchronization |
| `parking_lot_core 0.9.12` | `src/word_lock.rs:67` | Shared-state synchronization |
| `pin-project-lite 0.2.17` | `src/lib.rs:1057` | Process and terminal lifecycle support |
| `powerfmt 0.2.1` | `src/smart_display.rs:418` | Timestamp formatting |
| `rayon 1.12.0` | `src/vec.rs:148` | Concurrent scan pipeline |
| `rayon-core 1.13.0` | `src/latch.rs:50` | Concurrent scan pipeline |
| `regex-automata 0.4.18` | `src/util/wire.rs:272` | Name matching and hashing |
| `rusqlite 0.38.0` | `src/lib.rs:624` | OS and storage boundary |
| `rustix 1.1.5` | `src/backend/linux_raw/fs/syscalls.rs:79` | OS and storage boundary |
| `ryu 1.0.23` | `src/buffer/mod.rs:73` | Serialization and number output |
| `scopeguard 1.2.0` | `src/lib.rs:361` | Shared-state synchronization |
| `sd-notify 0.4.5` | `src/lib.rs:241` | Process and terminal lifecycle support |
| `serde_core 1.0.229` | `src/ser/impls.rs:821` | Serialization and number output |
| `serde_json 1.0.152` | `src/value/mod.rs:231` | Serialization and number output |
| `sha2 0.10.9` | `src/sha256/x86.rs:106` | Configuration parsing/hash support |
| `sharded-slab 0.1.7` | `src/shard.rs:316` | Logging and registry |
| `signal-hook 0.3.18` | `src/flag.rs:158` | Process and terminal lifecycle support |
| `signal-hook-registry 1.4.8` | `src/lib.rs:152` | Process and terminal lifecycle support |
| `smallvec 1.16.3` | `src/lib.rs:345` | Metadata allocation and containers |
| `thread_local 1.1.10` | `src/thread_id.rs:103` | Logging and registry |
| `time 0.3.55` | `src/util.rs:76` | Timestamp formatting |
| `toml_parser 1.1.5+spec-1.1.0` | `src/source.rs:47` | Configuration parsing/hash support |
| `tracing 0.1.44` | `src/span.rs:1604` | Logging and registry |
| `tracing-core 0.1.36` | `src/dispatcher.rs:314` | Logging and registry |
| `tracing-journald 0.3.2` | `src/socket.rs:22` | Logging and registry |
| `tracing-log 0.2.0` | `src/lib.rs:528` | Logging and registry |
| `tracing-subscriber 0.3.23` | `src/reload.rs:188` | Logging and registry |
| `utf8parse 0.2.2` | `src/lib.rs:70` | Terminal/CLI processing |
| `winnow 1.0.4` | `src/stream/token.rs:164` | Configuration parsing/hash support |
| `xxhash-rust 0.8.19` | `src/xxh3.rs:322` | Name matching and hashing |
| `zmij 1.0.23` | `src/stdarch_x86.rs:10` | Serialization and number output |

### Matches excluded from active Linux data-path claims

| Package | Verified reason |
|---|---|
| `anstyle-query 1.1.5` | Only syntax match is src/windows.rs:13; excluded on Linux. |
| `bitflags 2.13.2` | Unsafe bytemuck Pod/Zeroable impl macro at src/external.rs:236,243 is feature-gated; selected bitflags 2 features are std, not bytemuck. |
| `clap_complete 4.6.11` | Only unsafe is dynamic completion environment mutation src/env/mod.rs:226; env module requires disabled unstable-dynamic (src/lib.rs:63-66). Static shell completion remains. |
| `critical-section 1.2.0` | Contains unsafe primitives, but ratatui-core uses this only for not(feature="std") cache paths (layout/layout.rs:40,55,759). Current ratatui std cache uses thread_local; do not describe it as active Linux cache access. |
| `iana-time-zone 0.1.65` | Unsafe source is platform code (Windows/Darwin/Android/OHOS/Emscripten); current Linux backend src/tz_linux.rs contains no unsafe syntax. |
| `nu-ansi-term 0.50.3` | Only syntax match is src/windows.rs:22; excluded on Linux. |
| `num_threads 0.1.7` | Unsafe implementations are AIX/Apple/FreeBSD; Linux backend is safe procfs parsing. |
| `serde 1.0.229` | Unsafe matches are copies under src/core, included only for cfg(docsrs) (src/lib.rs:176ff). Production serde reexports serde_core at :252; count serde_core for runtime unsafe, not this duplicate. |
| `static_assertions 1.1.0` | Unsafe size-equality macro src/assert_eq_size.rs:79 creates a type-check-only uncalled closure. Compile-time type assertion, not an executable data-processing operation. |
| `tempfile 3.27.0` | Unsafe syntax is src/file/imp/windows.rs:66,82,93; Linux backend delegates to rustix without its own unsafe. |

The five token-only packages are `base64`, `clap_builder`, `serde_spanned`, `toml`, and `toml_datetime`; comments/lints mentioning unsafe do not make these unsafe implementations. The remaining 34 external packages have no unsafe token in their library src tree. Test/example-only matches were not promoted into runtime claims; embedded tests in an unsafe-bearing library do not add a separate package. Generated SQLite C is included explicitly, and Rust standard-library/compiler/native-libc internals are outside these Cargo-package counts.

Build scripts/procedural macros/platform/dev edges are excluded from the 121-package runtime closure, not from the supply-chain trust model. Examples outside it include `cc`, `pkg-config`, `vcpkg`, `autocfg`, `version_check`; `proc-macro2`, `quote`, `syn`, `serde_derive`, `clap_derive`, `thiserror-impl`, `tracing-attributes`; test tooling `proptest`, `insta`, `assert_cmd`; Windows/WASM/Android platform packages and unused terminal backends. Cargo metadata retains them because a lockfile covers more than this Linux runtime selection. No statement that all 284 external packages are executable daemon dependencies is warranted.

### HTTP removal / advisory limit

Cargo.lock search found no package entries named `openssl`, `openssl-sys`, `hyper`, `reqwest`, `rustls`, `native-tls`, `ureq`, `tiny_http`, `http`, `http-body`, `tokio`, `postgres`, or `tokio-postgres`. SHA-2 hashing remains for local configuration identity; it does not imply TLS. The normal runtime closure still includes POSIX socket/poll APIs for Unix IPC, journald, terminal and systemd integration, so dependency package presence alone does not prove which sockets are opened. Root review traces those first-party uses separately.

The threat model should name the package groups above as trusted native/unsafe dependencies and explain their data roles; lockfile pinning and cargo-deny policy reduce update risk but do not prove absence of bugs or hostile supply-chain code. No current RustSec advisory result is claimed, and this no-network review cannot certify advisory freshness.

## Proposed docs/security.md

The following text is intended to be copied verbatim into `docs/security.md`; it describes the reviewed source and explicitly retains unresolved limitations. It is a proposal, not a statement that its fixes or release gates have passed.

<!-- BEGIN proposed docs/security.md -->

# Tidemark security model

Tidemark observes filesystem allocation, directory entries, links, open-file metadata and storage capacity. It is designed for Rocky Linux 8 / XFS on LVM. Its scanning and watching code is designed to inspect metadata without reading regular-file payloads or modifying scanned data; the current boundary and atime limitations are stated below. It does not manage mounts or change LVM configuration. It stores its own snapshots and measurements, exposes a local Unix API, and can publish a metrics textfile. Explicit deployment commands also write Tidemark configuration and service enablement.

This document describes commit `fb8021cb14b1c31c2c6a6d46a7b3822bcf191cbb`. Known limitations below are part of this security model, not claims of completed remediation. The detailed source audit is `docs/reviews/W7-C1-audit2.md`.

## Trust and privilege

The installed `tidemarkd` service runs as root to observe protected directories and processes. Its capability bounding set contains exactly:

| Capability | Purpose |
|---|---|
| `CAP_SYS_ADMIN` | XFS bulkstat and fanotify initialization/mount observation on supported EL8 kernels. |
| `CAP_DAC_READ_SEARCH` | Traverse and inspect protected directories; open-by-handle metadata access in FID mode. |
| `CAP_FOWNER` | Open foreign-owned directories with O_NOATIME. Without it, the EPERM fallback can update directory atime. |
| `CAP_SYS_PTRACE` | Pass procfs access checks for other processes' fd/identity metadata. Tidemark does not attach to or trace those processes. |

Removing one of these capabilities from the current service can reduce measurement coverage or its no-atime behavior. They are substantial powers: a compromised root daemon is not confined to harmless metadata observation. A future privileged helper split would require separate design and testing.

The unit sets NoNewPrivileges, a 2 GiB memory limit for either supported cgroup generation, CPUQuota 200%, NOFILE 65,536, native syscall architecture, and namespace/realtime/personality restrictions. Its address-family allowlist is `AF_UNIX AF_NETLINK`. It intentionally does not create a private mount namespace: fanotify marks must observe the server's real mount objects. Do not add namespace-creating systemd hardening settings without validating watcher behavior. The unit is not a read-only filesystem sandbox.

Tidemark trusts the kernel, host procfs/sysfs, local filesystem implementation, installed binaries/libraries and reporting tools, root-controlled configuration, and administrators with mount or equivalent privilege. Another root process or an attacker controlling those components can defeat its invariants. Processes' comm/cgroup strings are labels, not authentication identities. Concurrent filesystem and process observations are best-effort measurements, not a transactional forensic record.

## Access and sudo

Production access is root only (ADR-018). There is no Tidemark access group. Administrators run the CLI with sudo; a filesystem permission error is not a reason to grant group access to the socket or snapshots. Default runtime and state directories are root-owned 0700, the API socket is 0600, and private state/configuration artifacts are intended to be root-owned with no group/world access. The metrics export is the deliberate exception described below.

The server validates socket ancestors, binds through a retained directory, checks socket/lock identities, and authenticates uid 0 using kernel SO_PEERCRED before parsing requests. Application authorization remains root only even if socket permissions are accidentally broadened. The library can operate on effective-user-owned temporary state for unprivileged fixtures; this does not change the production access policy.

Sudo authority should match the administrator's intended trust. Granting arbitrary Tidemark arguments includes selecting configuration/state/output paths and invoking deployment/purge functions; do not treat an unrestricted Tidemark sudo rule as a harmless read-only privilege. Use the installed absolute executable and trusted configuration, and avoid user-controlled environment or writable ancestors. Where delegation is necessary, constrain commands and arguments through the site's existing sudo policy. Do not use user-owned preseeded state.

## Information sensitivity and communications

Snapshots, catalog rows, process reports, metrics and logs can reveal raw file/path names, storage layout, process identities and activity. Names alone may disclose database, customer, backup or project information even though regular-file contents are never intentionally read. Protect state and backups with root-only ownership/access and the site's retention controls; SQLite snapshots are not encrypted by Tidemark.

Human result renderers escape control characters, backslashes and invalid UTF-8 in filesystem/process names. Printable Unicode can remain visually misleading. Some deployment and generic error diagnostics still bypass that escaping boundary; treat such text as untrusted. JSON uses proper JSON escaping and byte-path base64 fields where required. These are machine encodings: decoded JSON strings must not be interpolated into shells or terminals. Prometheus label encoding escapes quote, backslash and LF but does not make arbitrary label text terminal-safe.

The metrics textfile is published with mode 0644 for a collector. Its contents and the collector's network access are an explicit confidentiality decision: use a trusted collector directory and restrict downstream exporter/scraper access as appropriate. Tidemark does not provide a separate v1 redaction key. Journal access is governed by host journal permissions and retention; structured fields avoid newline-based field injection but do not make names nonsensitive.

ADR-020 removed HTTP, TLS listeners/clients and webhooks from v1. No webhook payload is sent, and no webhook/TLS configuration remains to secure. The only application listener is the local Unix API. Outbound Unix sockets are also used for journald and systemd notification, and Unix connection probes test the API socket. AF_NETLINK is allowed for local reporting integration; no first-party Internet socket path was found in the reviewed source. A separate node exporter may serve the metrics file over its own network endpoint; that service is outside Tidemark's API boundary.

## Read-only behavior and its limits

Walker and bulk scanning use directory entry enumeration and stat/ioctl metadata, no-follow component opens, filesystem/mount checks and bounded iterative traversal. Symlink analysis reads link text, not its referent's payload. Process inspection reads specific numeric procfs metadata paths, fdinfo and symlink targets; it does not open `/proc/PID/fd/N` for data. Watcher reads are from the fanotify notification-group stream. Delivered event descriptors are only inspected for metadata/path identity and closed unread; FID resolution uses O_PATH handles. LVM collection runs reporting-only `lvs`/`vgs` commands and never uses LVM mutation commands or the misleading `--readonly` option that hides thin-pool usage.

Configuration, Tidemark's own SQLite/JSON state, kernel procfs/sysfs, host OS identity and loader/runtime files are legitimate regular or pseudo-file reads. They must not alias scanned payloads. State snapshots can use SQLite mappings; a grep for Rust read calls alone cannot establish the global property. Scanning's metadata syscalls are subject to the Gate, and worker directory opens request O_NOATIME with a charged EPERM retry where needed. No user-supplied command is interpolated into a shell for storage observation.

The source gate in `xtask/src/checks.rs`, run by `cargo xtask ci`, rejects specified forbidden mutation symbols/commands. First-party unsafe is confined to reviewed `tidemark-sys` wrappers, with ABI/layout and parser/descriptor tests. These checks reduce mistakes; they are not a general proof of syscall behavior. The guest strace design in the second audit tracks fd provenance, inode aliases, dup/fork, fanotify-delivered fds and file mappings; its required assertion is zero attempted payload reads and explicit unread event-fd closure. That new regression design has not yet been implemented or run by this audit. Privileged XFS/fanotify/fault and cumulative fuzz gates remain required release evidence.

There are unresolved limits at the reviewed commit:

* Direct-mode state opens do not yet validate all ancestor/leaf trust. A symlinked database under user-controlled state can redirect SQLite outside the state directory. Daemon fd anchoring and nonregular-file refusal are stronger, but existing file/descendant ownership and marker provenance are incomplete. Use only fresh, trusted root-owned/private state beneath trusted parents; do not run privileged direct scans or stage user-preseeded state. A root-owned leaf under a user-owned replaceable parent is insufficient. The audit recommends one verified state-handle policy shared by direct mode, storage, daemon and deploy.
* Metrics publication validates and locks its temporary file but follows an unchecked collector parent. Use trusted, non-user-writable ancestry throughout; application enforcement needs the audit's parent-opener fix.
* Readlink can update the symlink inode's atime even when its parent directory was opened with O_NOATIME. Optional target stat can traverse other links/mounts. O_NOATIME fallback can also update a directory if privileges are inadequate. Consequently the current absolute no-metadata-write promise is not established. Resolve the contract through review and test noatime/relatime/strictatime behavior; Tidemark must never remount or restore timestamps to conceal these writes.
* On measured EL8 kernels, statx lacks mount IDs. Existing mountinfo skips and fd identity checks reduce crossings, but a newly introduced same-device, same-inode bind mount after a snapshot cannot always be distinguished in walking/name resolution. Trusted host mount administration is part of the boundary.

## Availability

The API limits active connection workers and request/response bytes, uses absolute frame/write deadlines, retains handler ownership and backs off on descriptor/memory accept pressure. Default directory/socket permissions and peer authorization exclude unprivileged slow API clients. Kernel or SQLite calls can still delay completion; handler ownership does not imply interruptible I/O.

Watcher reads have a descriptor budget per mount; the 512-descriptor daemon reserve is feasible only when the soft limit is at least `512 + 16 × mount_count`. Below that, the code warns and still uses the minimum batch. Overload becomes loss/degradation with bounded backoff. Rings, caches, dirty paths and foreground queues have count caps. Walk and bulk work use count/byte caps and explicit errors rather than claiming a complete truncated result. A local user can still create workloads that make measurements lossy, expensive or unable to complete.

Count caps are not a whole-process memory reservation. The current process collector retains full per-fd paths before deduplication and can exceed the 2 GiB service limit on a host permitting a sufficiently large ordinary-user fd inventory. Other queues and copies also consume memory. The second audit recommends aggregate byte budgets and fallible admission. Until that work and guest load testing are complete, Tidemark does not promise availability against a hostile local resource consumer. The service's restart policy is recovery, not prevention of denial of service.

## Deployment safety

`deploy stage` captures bounded no-follow regular configuration bytes, validates those exact bytes, hashes them and installs that same version. Blocking preflight checks happen before service/configuration changes; documented capability warnings alone can be skipped. Stage checks state locality, roots, mounts, space and deployment state. Config/state publication uses temporary files, sync and rename with retained handles where implemented. Source byte capture fixes the former separate-parse race.

After successful preflight, operations are not a transaction. A later failure attempts compensation for configuration, deployment record and service state, reports compensation failures, and can leave newly created empty directories or changed times. Service operations call a fixed `/usr/bin/systemctl` target without shell interpolation. Ordinary unstage stops/disables the service and removes its staged record; recursive state deletion requires the separate explicit purge guards. Purge traverses iteratively with no-follow descriptors, mount/identity checks and a depth limit. It is deliberately destructive to accepted Tidemark state, so do not grant it as read-only sudo authority.

Marker JSON identifies deployment state; its contents are not a substitute for trustworthy ownership. The state-provenance limitation above applies to stage/unstage/purge too. Existing fd anchoring is valuable but does not authenticate attacker-preseeded children. Complete the ownership checks before describing unsafe-state refusal as universal. Tidemark does not alter fstab, boot configuration, filesystem settings or LVM as part of normal observation.

## SELinux

Tidemark ships no SELinux policy and does not require a particular SELinux mode. Supplied deployment evidence reports successful Rocky 8.8 and 8.10 runs with enforcing SELinux, the four-capability unit, NoNewPrivileges and a 2 GiB limit, with no denials; the measured systemd security score was 5.9 MEDIUM. These guest results were supplied to the audit and not independently rerun. The owner's production servers do not use enforcing SELinux. Neither a supplied successful run nor permissive operation proves compatibility with every site's labeling/policy; investigate denials under the site's normal policy process rather than treating SELinux disabling as a Tidemark installation requirement.

## Supply chain and unsafe dependencies

The Rust toolchain is pinned to 1.98.0 and Cargo.lock records the dependency graph. The Rocky RPM Containerfile is a controlled build recipe with a pinned Rust/package helper version; its floating base image and repository downloads do not establish bit-for-bit reproducibility. Rebuild provenance and comparison remain necessary for that stronger claim. Build scripts, procedural macros, compiler, native libc/SQLite and OS tools are trusted supply-chain code even when they are not runtime Rust packages.

HTTP/TLS and PostgreSQL stacks are absent from the reviewed lockfile; SHA-2 remains for local configuration identity. The offline Linux CLI/daemon normal dependency selection contains 121 external package-version identities, versus 284 external identities across the full workspace/target/build/dev metadata graph. Package presence is not proof of linked code or a call to each function.

The following packages have unsafe library or macro implementations applicable or potentially instantiated in the selected Linux closure. The table deliberately includes supporting configuration/lifecycle packages and describes their relationship to observed metadata. It does not assert that all their unsafe functions consume hostile paths. Standard-library/compiler/native-library internals are additional trusted code. Version-specific source examples and platform/feature exclusions are in the second audit.

| Role | Packages (locked versions) | Data / justification |
|---|---|---|
| OS and storage boundary | `libc 0.2.190`, `libsqlite3-sys 0.36.0`, `linux-raw-sys 0.12.1`, `rusqlite 0.38.0`, `rustix 1.1.5` | Native syscall/ABI wrappers and SQLite FFI act on descriptor metadata, raw path bytes and persisted scan rows. Bundled SQLite C is a native dependency even though C has no Rust unsafe keyword. |
| Metadata allocation and containers | `allocator-api2 0.2.21`, `hashbrown 0.16.1`, `hashbrown 0.17.1`, `hashlink 0.11.1`, `smallvec 1.16.3` | Allocation/raw-table or inline storage holds metadata keys, links, records or cached SQL statements; hashbrown has two locked versions. |
| Concurrent scan pipeline | `crossbeam-channel 0.5.17`, `crossbeam-deque 0.8.8`, `crossbeam-epoch 0.9.21`, `crossbeam-utils 0.8.23`, `rayon 1.12.0`, `rayon-core 1.13.0` | Channels/work stealing/epoch reclamation and parallel sorting move and retain scan records. Unsafe memory ownership and synchronization are below first-party safe interfaces. |
| Shared-state synchronization | `lock_api 0.4.14`, `parking_lot 0.12.5`, `parking_lot_core 0.9.12`, `scopeguard 1.2.0` | Mutex/rwlock and guard implementation protects watcher, scan, API and collector state; unsafe cell access, locking invariants and cleanup primitives affect retained data. |
| Name matching and hashing | `aho-corasick 1.1.5`, `bstr 1.13.1`, `foldhash 0.2.0`, `memchr 2.8.3`, `regex-automata 0.4.18`, `xxhash-rust 0.8.19` | Byte/UTF-8 search, automata and hashing consume raw names, paths or parsed metadata. SIMD, unchecked indexing and raw byte loads make bounds/encoding invariants part of the trust boundary. |
| Serialization and number output | `itoa 1.0.18`, `ryu 1.0.23`, `serde_core 1.0.229`, `serde_json 1.0.152`, `zmij 1.0.23` | Metadata/paths and API/config/state fields are serialized or parsed. Unsafe UTF-8 construction, numeric conversion buffers and digit generation sit beneath public safe interfaces. |
| Display buffers and iterators | `castaway 0.2.4`, `compact_str 0.9.1`, `either 1.19.0`, `itertools 0.14.0`, `lru 0.18.5`, `num-traits 0.2.19` | TUI/report display and indirect iterator/container support handle derived metadata, geometry or formatted values. Their packages contain unchecked storage/transmutation/numeric or Pin helpers; package reachability is not proof each unsafe method is instantiated. |
| Terminal/CLI processing | `anstream 1.0.0`, `anstyle 1.0.14`, `anstyle-parse 1.0.0`, `clap_lex 1.1.1`, `crossterm 0.29.0`, `utf8parse 0.2.2` | CLI output/ANSI parsing, UTF-8 reconstruction, terminal ioctl and byte-oriented option handling. Some process only terminal state/options rather than scan names; terminal/report code shares their process and output pipeline. |
| Logging and registry | `lazy_static 1.5.1`, `log 0.4.34`, `matchers 0.2.0`, `once_cell 1.21.4`, `sharded-slab 0.1.7`, `thread_local 1.1.10`, `tracing 0.1.44`, `tracing-core 0.1.36`, `tracing-journald 0.3.2`, `tracing-log 0.2.0`, `tracing-subscriber 0.3.23` | Metadata-bearing diagnostics pass through logging fields, formatting/filtering, registries and thread-local caches. Unsafe lifetimes, erased pointers, initialization and journal socket ancillary-data code underlie safe APIs. |
| Timestamp formatting | `chrono 0.4.45`, `deranged 0.5.8`, `powerfmt 0.2.1`, `time 0.3.55` | Scan timestamps, scheduler clocks and rendered values use date/time types and formatting; unsafe invariants/range wrappers and local-time FFI exist in these packages. This does not mean arbitrary pathname bytes enter every unsafe function. |
| Error transport | `anyhow 1.0.104` | Erased error ownership/downcast implementation transports errors that can contain scanned paths. |
| Configuration parsing/hash support | `block-buffer 0.10.4`, `cpufeatures 0.2.17`, `generic-array 0.14.7`, `sha2 0.10.9`, `toml_parser 1.1.5+spec-1.1.0`, `winnow 1.0.4` | TOML source and parser slices plus config digest buffers. These process operator config, including configured roots/exclusions; cpufeatures chooses CPU routines, not scanned file payload. No cryptographic network/TLS role remains. |
| Process and terminal lifecycle support | `errno 0.3.14`, `getrandom 0.4.3`, `mio 1.2.4`, `pin-project-lite 0.2.17`, `sd-notify 0.4.5`, `signal-hook 0.3.18`, `signal-hook-registry 1.4.8` | Errno FFI, entropy for temporary state names, terminal poll/notification, systemd status and signal shutdown infrastructure. These packages contain unsafe but are not direct scanned-payload consumers. pin-project-lite defines unsafe projection macros; no asynchronous runtime is used by Tidemark. |

Platform-disabled or nonruntime-only matches excluded from that table are anstyle-query 1.1.5, bitflags 2.13.2's disabled bytemuck impls, clap_complete 4.6.11's disabled dynamic completion, critical-section 1.2.0's non-std cache route, iana-time-zone 0.1.65, nu-ansi-term 0.50.3, num_threads 0.1.7, serde 1.0.229's docs-only duplicate core, static_assertions 1.1.0's uncalled type-check closure and tempfile 3.27.0's Windows implementation. Runtime serde_core and rustix remain included. Unsafe in proc-macro/build/dev/other-platform packages must still be considered when evaluating the broader supply chain.

Cargo-deny policy checks advisories, sources and licenses, but the current ordinary CI command can skip it when absent and the provisioned deny workflow remains explicitly nonblocking pending rollout. `--require-deny` makes missing tooling fatal. Resolve/disposition existing failures and make the intended security gate required before claiming it protects every accepted change. This offline audit did not refresh an advisory database or establish a clean cargo-deny result; pinning does not prove absence of vulnerabilities.

## Deliberate exclusions

Tidemark does not defend against root-equivalent attackers, a hostile kernel/filesystem implementation, substituted trusted binaries/reporting tools, or an administrator who grants access to private state and then expects it to remain confidential. It does not sanitize arbitrary output for downstream shell execution, provide transactional consistency over a changing filesystem, infer reliable security identity from process labels, encrypt snapshots, or guarantee uninterrupted measurement under hostile resource exhaustion. Current state/metrics trust gaps remain bugs to repair, not reasons to redefine all user-controlled paths as safe. No PostgreSQL-specific tracking, HTTP service or webhook transport is part of v1.

<!-- END proposed docs/security.md -->

## Validation performed and evidence retained

All source checks and builds refer to `fb8021cb14b1c31c2c6a6d46a7b3822bcf191cbb`. Commands used the installed pinned 1.98.0 toolchain and a temporary Cargo home linked to the existing cache; no network access was attempted. The following commands were each wrapped in `timeout 1500`:

```sh
cargo test --offline --locked -p tidemark-sys
cargo test --offline --locked -p tidemark-sys --features privileged --no-run
cargo build --offline --locked -p tidemark-store
cargo clean
```

| Evidence | Observed result / practical limit |
|---|---|
| `library/sys-tests.log` | 13 unit tests passed; zero failures/ignored tests. Includes XFS layout/ioctl constants and fanotify descriptor cleanup. The non-feature integration target ran zero tests. |
| `library/sys-privileged-compile.log` | Privileged sys tests compiled successfully; **pending VM**, none executed. This is not compilation of every workspace privileged target. |
| `library/store-build.log`, `state-alias-probe.rs`, `state-alias-probe.log` | Current store library built offline. A standalone fixture linked that exact library and opened a symlinked `timeseries.db`; the separate empty temporary referent became a 61,440-byte SQLite database. No root/victim account or real CLI/daemon deployment was used. |
| `library/readlink-atime.json` | Fresh local temporary symlink experiment with a NOATIME parent observed atime change. This is host syscall behavior, not a Rocky/XFS privileged result. |
| `library/metadata.json`, `linux-runtime-tree.txt`, `dependency-inventory.json` | Offline locked dependency inventory and cached-source classifications; 121 external Linux normal package-version identities. No advisory freshness or binary-linkage proof. |
| `library/cargo-clean.log` | `cargo clean` succeeded before submission, removing 1,881 files / 689.0 MiB of build artifacts. Source/evidence files were retained. |
| `library/candidate-review.md` | Separate exact-`b75b79b` CI-shellcheck architecture evidence: one advisory-output visibility NIT, no claim about full production security of that older candidate. It is outside this report's security finding count. |

Evidence is retained under this attempt's output directory, `attempt-8ccb98497f8a58ae51d98b7ad6762351914c4b56aaddb32b5d0f22c9b21e6ac6/library/`; the attempt summary is `report.md`. The final report was checked for whitespace errors, complete table structure and source-reference mistakes. `cargo fmt --all -- --check` also passed; its empty success log is `library/fmt-check.log`. Receipt submission records review completion and evidence only; it does not accept findings or certify the task/release.

## What was not checked

No Rocky VM was available to this review. No guest trace, real fanotify mark/bulkstat, mount/bind race, root ownership transition, LVM mutation, deployment/purge, service restart/reboot, SELinux enforcement trial, cgroup OOM, or low-NOFILE flood was executed. The supplied Rocky 8.8/8.10 facts were not independently reproduced. No Miri, sanitizer, new fuzz run or 24-hour cumulative fuzz gate was performed. The strace controller/checker is a design, not an implemented test. The metadata-read conclusion is source-based; dependency/SQLite/kernel/loader behavior needs that runtime oracle.

Full workspace CI, clippy, cargo-deny and the existing core/edge/deploy regression suites were not rerun for this report-only change. No fresh vulnerability database, complete third-party unsafe audit, cryptographic/reproducible-build assessment, systemd seccomp proof, or whole-process peak-RSS measurement is claimed. Native dependency inventory is conservative source/package classification, not compiled-function reachability. Reviewed no-follow/race invariants assume the host kernel/filesystem contract and ordinary unprivileged attackers, not hostile root.

The audit did not reprove hard-link accounting, all SQL/query correctness, every scheduler/alert semantic edge, or the project's broader production performance gates. No PostgreSQL work was added or reviewed. Remaining findings need separately scoped implementation, contract resolution where noted, review and VM evidence; only this report is delivered here.

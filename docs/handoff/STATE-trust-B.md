# Handoff — STATE-trust-B: trusted deployment, daemon admission and metrics directories

## Needs from coordinator

Independent review and disposable Rocky guest acceptance remain pending. Run the real second-user tests and commands below in a guest; the workstation sandbox maps only uid 0 (`/proc/self/uid_map` is `0 0 1`). A focused non-root attempt failed at `setpriv: setresuid failed: Invalid argument`, before executing tests. Unix listener and namespace scenarios require an unrestricted runner. CI skipped cargo-deny because it is not installed; advisory/license acceptance needs an equipped runner. This is implementation/validation evidence, not verified task success or release approval.

- Attempt: attempt-ac90d9a1ca914907a659958f2a860a12137819034ad28ed91d7a1aa20dfd5893
- Baseline: `45fc3d6d0d346b3963e2f55df7c7361248ce7931`
- Branch: supplied `hp-ac90d9a1ca914907a659958f2a860a12137819034ad28ed91d7a1aa20dfd5893`
- Date: 2026-10-13
- Status: evidence candidate; root/second-user acceptance pending VM.

## What changed, by coordinator decision

1. **Shared ancestry:** deployment/configuration directory opens delegate to `tidemark_store::state_dir::trusted_directory`. Missing directory creation retains the nearest verified ancestor and iteratively creates at most 4096 missing components, checking each resulting descriptor. Directory creation retains NOATIME with EPERM fallback. Daemon config reads use the same included helper. Stage retains the installed configuration parent for its old-content read, backup check, configuration/backup writes, compensation and cleanup; `Env::config_write` supplies the existing failure-injection boundary with an already checked parent. The path is not resolved again for publication. A deterministic fake-environment checkpoint renames that parent and substitutes a symlink immediately before publication; the original retained directory receives the configuration and the target stays empty.
2. **Markers do not waive ownership:** state and deploy always require euid ownership. State group/world write bits are always refused. A checked marker authorizes only normalizing state read/execute bits; deploy remains private even with a marker. Marker reads use `StateDir::open_leaf`, which checks regular type, euid owner, single link and absence of group/world write bits on the opened file. Marked normalization only calls fchmod(0700); it no longer chowns existing state. Tests retain marked 0750 acceptance/normalization and unmarked 0750 refusal, and add marked 0770 and simulated foreign-owned state/purge refusal.
3. **Existing content before mutation:** `state_handles` checks DB main/WAL/SHM/journal, `.bak-v0` and all its sidecars, `scan.lock`, marker, staged/firstboot records and snapshots/tmp/deploy through `StateDir`, using descriptor anchors reopened by the supported `/proc/self/fd/<n>/.` interface. Absent entries remain optional. Existing record reads retain the original 1 MiB bound; oversized records refuse admission. Existing content is checked before a deploy child is created, including the standalone private-directory helper. `complete_handles` repeats admission before creating a missing deploy child or normalizing the state root. Daemon preflight uses this shared inventory, replacing type-only checks that ran after directory completion. Its existing state/child mount guards are preserved. Snapshot-root descendants are left to part A when used; there is no startup tree walk. Firstboot/verify state reads now use checked storage leaves instead of the generic config reader. Trust diagnostics add original escaped paths, owner, mode and link count; part A's error interface otherwise remains unchanged.
4. **Purge:** the state parent is opened through `trusted_directory`; the retained root must satisfy euid ownership/no foreign writes, and its marker is opened with the leaf rule. Existing marker/config/root matching, mount IDs, bounded walk, retained inode identity checks and relative unlink remain. User-facing rustdoc says ownership-checked marker, not authentic marker. Purge does not infer authenticity from marker JSON.
5. **Metrics:** every export pass opens the collector with `trusted_directory`, retaining that descriptor for staging open, rename and directory sync. Refusal happens before staging opens or chmod and is reported by the existing degraded/retry loop. Existing staging regular/euid/single-link, no-follow, flock and named-inode checks remain. Tests cover writable non-sticky ancestry, symbolic-link replacement after one successful pass, untouched target and recovery after restoration.
6. **Paths:** eligibility and added trust diagnostics use the established lossy byte display helper. Existing installation/output-path diagnostic sites in owned files use it too. A regression checks invalid UTF-8 and newline bytes are escaped without injecting a newline into the eligibility error.

The shared deployment commit combines decisions 1/2 and stage's decision-3 adapter because the marker opener, retained StateDir checks and ancestry must agree. Separate commits cover purge, daemon admission/firstboot, metrics, and path-display evidence. No branch creation, merge, push, dependency, manifest, core/API contract, plan or security-document edit occurred. `api.rs` was deliberately not changed: it could later share the ancestry helper, but its stricter private socket-directory policy and listener tests must be preserved.

## Final rules

| Who checks what | When | Refused when / accepted behavior |
|---|---|---|
| Shared trusted_directory: configuration/deployment/purge/metrics ancestry | Each configured-directory open | Symlinks or nondirectories; parent traversal; owner neither root/euid nor namespace overflow owner, except read-only ancestry; group/world writable without sticky bit. Sticky children obey the part-A2 ancestry table, including propagated sticky chains. Read-only ownership exception never excuses unsafe write modes. |
| Deployment and daemon configured state | Before stage/service mutation, and completion recheck | Not euid-owned; any group/world write bit; any group/world permission without an honored marker. Honored marker allows read/execute bits to be normalized to 0700. No ownership repair. |
| Deploy child | Before records or service mutations | Not euid-owned or any group/world permission. Marker does not relax this rule. |
| Marker and all top-level persistent leaves | Before stage/start, again on checked record reads | Not regular, foreign owner, group/world write bit, or link count != 1. Symlinks/FIFOs refuse promptly without content reads. Read bits allowed by A2; new leaves are 0600. |
| snapshots and tmp | Before stage/start | Used subdirectory rule: nondirectory/symlink, foreign owner, group/world write bits. Read/execute bits allowed; no normalization. Daemon retains previous child mount rejection. Snapshot descendants checked by store when used. |
| DB inventory | Before stage/start | `timeseries.db`, `-wal`, `-shm`, `-journal`, `.bak-v0`, `.bak-v0-wal`, `.bak-v0-shm`, `.bak-v0-journal`; same leaf rule. `scan.lock` uses it too. |
| Installed config/source | Read and stage publication | Trusted directory ancestry, no-follow/nonblocking regular bounded reads; capture/parse/hash same bytes. Config bytes are administrator-selected input; no new leaf-owner rule is imposed on source/config files. Publication and rollback use the retained installed parent. |
| Purge root/marker | Before stop/delete | Untrusted parent ancestry, foreign root owner, unsafe root write bits or unsafe marker leaf; marker/config state paths must match. Existing mount/depth/identity safeguards retained. |
| Metrics collector | Once each pass | Untrusted ancestry, symlink or replaceable non-sticky writable directory. Degraded with reason; no create/chmod on refusal; next pass retries. Verified fd used for staging/rename/sync. |

`StateDir`'s retained-anchor reopen is the published part-A interface, not a traversal of procfs ancestry. Its final directory and leaf policy is rechecked. These controls assume a trusted filesystem/kernel and exclude hostile root or same-euid mutation; checks followed by SQLite pathname opens retain part A's documented limit. Added diagnostic-only lstat after directory refusal is not used as authorization.

## Regression evidence

- Deployment `marker_never_waives_owner_or_write_permissions`: valid-format marker in simulated foreign state, and marked 0770, refuse stage/purge before service calls; bytes/mode preserved.
- `unsafe_marker_and_state_content_are_refused_without_changes`: 7 names × 5 hostile cases (symlink, hard link, FIFO, group-write, simulated foreign owner), with original path/owner/mode/link diagnostics, unchanged sentinel bytes/mode and no service queries/mutations. Names include marker, DB/WAL/journal/lock and both deployment records.
- Existing `marker_authorizes_state_normalization_only_after_deploy_is_eligible` and unmarked permission tests still pass. One existing octal-format assertion now accepts the shared helper's unpadded octal; ownership and unchanged-permission assertions remain. Firstboot's corresponding privileged assertion also accepts this formatting. The w6a1 nonprivate-state assertion accepts an earlier shared-ancestry write refusal and now explicitly requires original path, owner, mode and link-count diagnostics.
- `stage_refuses_writable_ancestor`, `config_publication_retains_verified_parent_after_path_replacement`, `eligibility_error_escapes_non_utf8_and_control_path_bytes` cover ancestry, retained config publication and terminal-safe diagnostics.
- Daemon unit `preflight_refuses_foreign_database_leaves_before_mutation`: simulated foreign DB/WAL/journal refused before deploy/lock creation, original bytes preserved.
- NEW ordinary-user-capable `tests/w6a7.rs`: actual daemon binary refuses 5 persistent names × 4 hostile kinds, no deployment child or extra state entries, sentinel bytes/mode unchanged; separate state-ancestor group-write refusal. These fail before listener/service initialization.
- Metrics `metrics_refuses_writable_ancestry_without_creating_or_changing_files` and `exporter_rechecks_directory_each_pass_and_recovers_after_symlink_replacement` cover unchanged directory mode, zero writes on refusal, degraded health and recovery.
- New guarded privileged tests, **compiled only; pending VM**: CLI `real_foreign_owner_marker_cannot_authorize_root_stage_or_purge`; daemon `real_foreign_database_owner_refused_before_daemon_mutation`; metrics `metrics_refuses_real_foreign_owned_ancestor`. Each requires `TIDEMARK_DEPLOY_TEST_VM=1` and uid 0, and changes ownership only inside its disposable fixture.

## Manual fixture review

Read CLI `tests/w4b.rs`, `w4b/mod.rs` and `w4b/privileged.rs`: state is absent/private under owned private tempfile ancestry; malformed config and missing markers block before service commands; successful VM fixtures stage their own state before unstage/reboot verification. Read `tests/rules.rs`, `e2e/support.rs` and e2e scan setup: dropped-user helpers transfer their own temp/state to that user and keep private state, namespace fixtures create state with mapped euid and safe parent modes; scanned mode-000 directories are not state ancestry. The sandbox does not supply a second mapped uid or new namespace.

Read daemon binary/lifecycle fixtures in `w6a1`, `w6a2/runtime`, `w6a3/operator`, `w6a4`, `w6a5` and w6b fixtures: configured state is absent/private, API parents explicitly 0700, config/metrics files under owned non-writable temporary ancestry; unmarked state/deploy 0750/0777 and metrics staging 0666 are intentional refusal cases. Firstboot/scheduler fixtures explicitly make deploy 0700 where present. Storage readability at umask 022 remains accepted under A2; daemon/deployment configured-state privacy remains stricter. No out-of-scope fixture was edited. Socket-denied conditional returns are not listener evidence.

## Disposable guest commands (never run these on the workstation)

Use an installed candidate in a disposable Rocky guest with a real existing second user `tmtrust`; run as root. All paths below are disposable test fixtures. Do not use production state/service data. Run the three guarded tests separately too:

```sh
export TIDEMARK_DEPLOY_TEST_VM=1
 timeout 1500 cargo test --offline -p tidemark-cli --features privileged --lib real_foreign_owner_marker_cannot_authorize_root_stage_or_purge
 timeout 1500 cargo test --offline -p tidemarkd --features privileged --lib real_foreign_database_owner_refused_before_daemon_mutation
 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6b metrics_refuses_real_foreign_owned_ancestor
```

Pre-seeded foreign state with forged marker before stage:

```sh
set -eu
umask 022
guest_user=tmtrust
id "$guest_user"
guest_home=$(getent passwd "$guest_user" | cut -d: -f6)
guest_work=$(mktemp -d /tmp/tidemark-trust-B.XXXXXX)
chmod 700 "$guest_work"
guest_state=$(runuser -u "$guest_user" -- mktemp -d "$guest_home/tidemark-trust-B.XXXXXX")
python3 - "$guest_work/source.toml" "$guest_state" "$guest_work/api.sock" <<'PY'
import sys
with open(sys.argv[1], 'w') as f:
    f.write('[general]\nstate_dir="'+sys.argv[2]+'"\nsocket="'+sys.argv[3]+'"\n')
PY
runuser -u "$guest_user" -- python3 - "$guest_state" "$guest_work/installed.toml" <<'PY'
import json, os, sys
state, config = sys.argv[1:]
with open(os.path.join(state, '.tidemark-state.json'), 'w') as f:
    json.dump({'format':'tidemark-state-v1','state_dir':list(os.fsencode(state)),
               'config_path':list(os.fsencode(config))}, f)
PY
stat -c '%u %a %h %s' "$guest_state" "$guest_state/.tidemark-state.json" > "$guest_work/before"
sha256sum "$guest_state/.tidemark-state.json" > "$guest_work/hash-before"
rc=0
timeout 15 tidemark -c "$guest_work/installed.toml" deploy stage --from "$guest_work/source.toml" > "$guest_work/stage.out" 2>&1 || rc=$?
test "$rc" = 5
cat "$guest_work/stage.out"  # must name foreign ancestry/owner; no service action
stat -c '%u %a %h %s' "$guest_state" "$guest_state/.tidemark-state.json" > "$guest_work/after"
sha256sum "$guest_state/.tidemark-state.json" > "$guest_work/hash-after"
cmp "$guest_work/before" "$guest_work/after"
cmp "$guest_work/hash-before" "$guest_work/hash-after"
test ! -e "$guest_work/installed.toml"
test ! -e "$guest_state/deploy"
```

Hard-linked DB before daemon start; reuse root-owned `guest_work`:

```sh
mkdir -m 700 "$guest_work/state"
printf 'untouched\n' > "$guest_work/sentinel"
chmod 600 "$guest_work/sentinel"
ln "$guest_work/sentinel" "$guest_work/state/timeseries.db"
cat > "$guest_work/daemon.toml" <<EOF_CONFIG
[general]
state_dir="$guest_work/state"
socket="$guest_work/api.sock"
[sampler]
mounts=[]
[watch]
enabled=false
mounts=[]
[lvm]
enabled=false
[procscan]
enabled=false
[metrics]
textfile=""
EOF_CONFIG
stat -c '%u %a %h %s' "$guest_work/sentinel" > "$guest_work/db-before"
sha256sum "$guest_work/sentinel" > "$guest_work/db-hash-before"
rc=0
timeout 15 tidemarkd --config "$guest_work/daemon.toml" > "$guest_work/daemon.out" 2>&1 || rc=$?
test "$rc" = 1
cat "$guest_work/daemon.out"  # must name timeseries.db and link-count refusal
stat -c '%u %a %h %s' "$guest_work/sentinel" > "$guest_work/db-after"
sha256sum "$guest_work/sentinel" > "$guest_work/db-hash-after"
cmp "$guest_work/db-before" "$guest_work/db-after"
cmp "$guest_work/db-hash-before" "$guest_work/db-hash-after"
test ! -e "$guest_work/state/deploy"
rm "$guest_work/state/timeseries.db"  # remove this test alias only
```

Metrics under a second user's home, while daemon state and socket remain root-owned:

```sh
guest_collector=$(runuser -u "$guest_user" -- mktemp -d "$guest_home/tidemark-metrics-B.XXXXXX")
python3 - "$guest_work/daemon.toml" "$guest_collector/tidemark.prom" <<'PY'
import sys
p, output = sys.argv[1:]
s = open(p).read().replace('textfile=""', 'textfile="'+output+'"\ninterval="100ms"')
open(p, 'w').write(s)
PY
tidemarkd --config "$guest_work/daemon.toml" > "$guest_work/metrics-daemon.out" 2>&1 &
guest_pid=$!
# Wait up to ten seconds for the root-owned socket and metrics degraded status.
for guest_try in $(seq 1 100); do
    tidemark -c "$guest_work/daemon.toml" --json status > "$guest_work/status.json" 2>/dev/null &&
    python3 - "$guest_work/status.json" <<'PY' && break
import json, sys
s=json.load(open(sys.argv[1]))['data']
assert any(c['name']=='metrics' and c['state']=='degraded' and 'owner' in c['detail'] for c in s['components'])
PY
    sleep 0.1
done
kill -TERM "$guest_pid"
wait "$guest_pid"
python3 - "$guest_work/status.json" <<'PY'
import json, sys
s=json.load(open(sys.argv[1]))['data']
assert any(c['name']=='metrics' and c['state']=='degraded' and 'owner' in c['detail'] for c in s['components'])
PY
test -z "$(find "$guest_collector" -mindepth 1 -print -quit)"
# Remove only these disposable guest fixtures after retaining their evidence.
runuser -u "$guest_user" -- rm -rf -- "$guest_state" "$guest_collector"
rm -rf -- "$guest_work"
```

## Proposed plan and docs/security.md text (not applied)

Plan: Deployment, daemon admission and metrics directory opens share the store's trusted ancestry policy, including namespace/read-only exceptions and sticky chains. Existing state requires euid ownership; a checked regular euid-owned single-link non-group/world-writable marker may authorize only normalization of state read/execute permissions, never ownership or write permissions. Deploy stays private. Before stage/start, verify top-level database and migration-backup sidecars, lock, marker/records and snapshots/tmp/deploy. Snapshot descendants are verified when used. Config publication/backup/rollback, state operations, purge and metrics publication use verified retained descriptors. Unsafe metrics collector ancestry degrades that component and retries next pass without creating or chmodding the collector. Root-only installed access and API socket privacy remain unchanged.

Security: Marker JSON identifies a state directory; it is not authenticated proof and does not waive provenance checks. Privileged configured paths must not sit beneath another user's mutable directory. Symlinks, unsafe write permissions, foreign-owned state/records and hard-linked persistent leaves refuse before state mutation. Readable existing storage is accepted by the storage integrity policy with its existing once-per-process warning; configured daemon/deploy state remains private unless a trusted marker permits normalization. Metrics verifies its directory every pass and retains the fd through publication and sync. These defenses exclude hostile root/same-euid processes and do not prove arbitrary SQLite VFS race safety.

## Assumptions and scope

- Coordinator decisions and the corrected part-A2 table govern policy. No questions were asked, no memory projection was edited, and canonical receipts had no pending delivery at the initial and implementation checkpoints.
- Config/source regular leaves preserve their existing bounded-read policy; new ownership/link rules apply to persistent state and markers. Source/config ancestry is deliberately stricter, so privileged stage may refuse a source beneath an ordinary user's home.
- Real second-user, user-namespace, mount and listener behavior is not inferred from sandbox harness pass counts. Tests use hard links, FIFOs, symlinks and modes without root-only operations; real ownership tests are feature-gated and VM-guarded.
- All builds use the pinned Rust 1.98.0 toolchain and offline Cargo cache copied into `/tmp/trust-cargo`, with four build jobs. No dependency was fetched/added. Long Cargo commands have timeout 1500. No host sudo, mount, filesystem formatting, LVM mutation, service action or system-directory write was performed.
- Git printed a pack-index-unavailable warning while committing, but created the commits. Final submission traversal/receipt and clean status are recorded separately; no shared pack/history repair was attempted.

## Validation

Final command results and harness totals below come from the retained transcripts. The initial full umask-022 run found two regressions: a dropped preflight record-size bound in verify and an old daemon permission-error wording assertion. The checked record-size bound was restored (also retained for daemon/stage admission), the assertion strengthened, and both focused reproductions were rerun. Early diagnostic runs superseded by final checks are not final-source evidence. Privileged compilation is not execution. Tests that conditionally return after denied socket/namespace operations do not establish those scenarios.


| Required check | Observed result | Transcript in attempt library/ |
|---|---|---|
| `cargo fmt --check` | Exit 0 | final-fmt.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 | final-clippy.log |
| Same Clippy command with `--features privileged` | Exit 0; no tests executed | final-clippy-privileged.log |
| `cargo test --offline --workspace --no-fail-fast`, explicit umask 022 | Exit 0; **822 passed, 0 failed, 14 ignored**, 79 harness summaries | final-workspace-022.log |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compile only | final-privileged-build.log |
| `tools/ci.sh`, explicit umask 022 and offline Cargo | Exit 0; **822 passed, 0 failed, 14 ignored**, 79 summaries; source policy, format, Clippy, tests and privileged compilation passed; cargo-deny explicitly skipped as unavailable | final-ci.log |
| Checked record-bound reproduction | 1 passed | record-bounds.log; full final workspace also passed |
| Daemon nonprivate-state assertion reproduction | 1 passed | nonprivate-state.log; full final workspace also passed |
| Focused deployment then new escaped-path regression | 38 passed plus 1 passed | deploy-final.log, path-display.log; full final workspace includes all 39 deployment tests |
| Focused daemon library / w6a7 / w6b | 89 / 2 / 15 passed, 0 failed | daemon-final.log; full final workspace supersedes this earlier run |
| Four documented guest shell snippets | Each passed `bash -n`; none executed | guest-syntax.log and guest-1.sh through guest-4.sh |
| Actual non-root focused run attempt | Did not start tests: `setpriv: setresuid failed: Invalid argument` | nonroot-deploy.log |

Counts sum Cargo harness summaries, including helpers and conditional socket/namespace returns. They do not establish listener, namespace, real second-user or privileged runtime scenarios. The initial workspace transcript had 2 failing targets; it is diagnostic only, superseded by the final serialized workspace and CI passes. No privileged-feature test was executed. No dependency was fetched or added. Shared manifest/module-declaration edits: none.

Final Rust source is `f35d6d77efcbe79c670e552ea35dbb2a08e1c6a7`; subsequent changes only finalize this note. The exact check script and start/exit timestamps are in `library/final-checks.sh` and `final-checks-status.log`. Final cleanup, clean Git status and submission receipt are retained in the external attempt report/artifacts so this committed note does not imply independent acceptance.

# POST-absentmount — distinguish configured absent mounts

## Needs from coordinator

- Independent review and uid-1000/socket-capable and GitHub ubuntu-24.04 gates remain required. This worker runs as uid 0 in a socket-restricted sandbox.
- Privileged runtime checks remain pending VM; only feature-enabled compilation is authorized here.

## What changed

API admission now distinguishes an unconfigured requested mount from a configured mount explicitly published absent (`Some(None)`). The latter returns `mount not present: /A` through the existing `DaemonError::Unavailable` path. Unknown presence and present configured mounts retain their previous admission behavior. No report method or internal caller gains an admission check; the existing five API dispatch checks remain the only callers of this helper.

The daemon rejection helper now checks both unconfigured paths (including a subdirectory) and configured absent paths for hot, now, phantom, writers and lvm. A socket-free CLI unit test checks absent-mount exit 4 and empty output for each command in human and JSON modes. Existing five-command unconfigured CLI coverage still requires exit 5 and its error JSON. Reviewed `tests/w6a3.rs::hot_unwatched_mount_is_precondition_in_text_and_json`: its fake response describes an unconfigured mount, so its existing expectations correctly remain unchanged.

Both interface and operator documentation qualify daemon-less exit 4: now/hot resolve `--mount` before connecting, so missing paths exit 5 with local `not_found` JSON even without a daemon. Other local resolution errors likewise take precedence. No code change implements N1.

## Before / after output

Let `S` be the configured socket and `D` be `/A is not watched (watch.mounts = ["/A"]); add it to [watch].mounts and run: tidemark deploy stage --from <file>`. With a connected daemon and an existing directory `/A` configured but published absent:

| Command | Before human stderr | Before JSON stdout schema | After stderr (human and JSON) | After stdout (human and JSON) | Exit before → after |
| --- | --- | --- | --- | --- | --- |
| hot | `hot: S: D` | `tidemark.hot.v1` | `hot: S: daemon request hot: unavailable: mount not present: /A` | empty | 5 → 4 |
| now | `now: S: D` | `tidemark.now.v1` | `now: S: daemon request now: unavailable: mount not present: /A` | empty | 5 → 4 |
| phantom | `phantom: S: D` | `tidemark.phantom.v1` | `phantom: S: daemon request phantom: unavailable: mount not present: /A` | empty | 5 → 4 |
| writers | `writers: S: D` | `tidemark.writers.v1` | `writers: S: daemon request writers: unavailable: mount not present: /A` | empty | 5 → 4 |
| lvm | `lvm: S: D` | `tidemark.lvm.daemon.v1` | `lvm: S: daemon request lvm: unavailable: mount not present: /A` | empty | 5 → 4 |

Each stderr line ends in a newline. Before JSON contains `data.error.code = "unavailable"` and `data.error.message = "unavailable: D"`; JSON mode also emits the diagnostic on stderr. After JSON emits no document, matching existing remote-unavailable handling. Output is established by dispatch/CLI mapping tests and inspection of the outer command context, not a live unmount experiment.

For an unconfigured path `/B`, all five commands retain `D` with `/B` in its first position, exit 5, the same schemas and error fields, and the same human diagnostic. No config-edit advice is emitted for a configured absent mount.

## Regression evidence

Before the production fix, four existing API tests updated to expect the absent detail failed with the old add-it message (now/lvm/phantom/writers). The initially added hot case also failed because the test request omitted required hot fields; that fixture was corrected before the passing run. After the fix, all five rejection tests pass (15 dispatch cases total), using injected time 120 and explicit presence publication at time 0. CLI coverage checks ten absent-mount mappings and ten unconfigured mappings across JSON/human modes.

## Assumptions

- Only explicit published absence means absent; missing presence retains existing startup behavior.
- Existing unavailable JSON behavior (empty stdout) is intentional, as required by the task; no new error document or CLI contract was introduced.
- No live mounts or privileged host operations are needed for this metadata/state diagnostic correction.

## Scope and deviations

Only owned paths changed. No dependencies, manifests, shared module declarations, unsafe code, host configuration changes, branches, merges or pushes. No memory projections edited. No privileged runtime tests added because this change is fully exercised through injected daemon state and CLI error mapping; existing privileged tests are build-only here.

## Validation

All commands use pinned Rust 1.98.0, host `CARGO_HOME=/home/brewerm/.cargo`, offline mode (`CARGO_NET_OFFLINE=true`, plus explicit `--offline` for direct Cargo commands), uid 0, and `timeout 1500` per long command. Logs are retained in the attempt output `library/`.

| Check | Result | Log |
| --- | --- | --- |
| `cargo fmt --check` | exit 0 | `cargo-fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `cargo-clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0; no failed targets | `cargo-test.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; build only | `cargo-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `cargo-doc.log` |
| `sh tools/ci.sh` | exit 0; all available checks passed; cargo-deny unavailable | `sh-tools_ci.sh.log` |

Relevant primary test summaries: CLI unit 163 passed / 1 scale ignored; daemon unit 135 passed; CLI w6a3 integration 34 reported passed; daemon w6a3 integration 13 reported passed. Nested child-process summaries are not added to these counts. Socket probes return early where unavailable, and root can bypass permission fixtures; these totals do not establish uid-1000 or socket-capable execution. The new regression tests require neither sockets nor filesystem privilege. Targeted rejection run: 5 passed / 0 failed, see `after.log`; failing pre-fix evidence is `before.log`.

Privileged runtime: pending VM. No privileged test binaries were executed. Independent review and platform gates remain outstanding.

CI wrapper source policy: 150 owner tags, forbidden-symbol scan passed; strict-ancestry tests: 2 passed. `cargo-deny` is not installed and the wrapper explicitly skipped it; dependency/advisory verification remains for the coordinator. Final `git diff --check` passed. Build artifacts are removed with `cargo clean --offline` after committing.

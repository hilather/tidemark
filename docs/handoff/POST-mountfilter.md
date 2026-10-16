# Handoff — POST-mountfilter

## Needs from coordinator

- Independently run the socket-capable uid 1000 / umask 022 gate and GitHub ubuntu-24.04 checks. This worker is uid 0 in a socket-restricted sandbox.
- Privileged runtime checks remain pending VM; only compile privileged tests here.

## What changed

The daemon API dispatch validates optional mount filters for `now`, `phantom`, `writers`, and `lvm` with the same helper used by `hot`. Renamed the helper to `unwatched_mount_detail` and generalized its argument to an optional mount label. Its exact wording, configured-list formatting, path comparison, and configured-but-absent behavior are preserved. State view methods and internal callers are unchanged.

The five CLI daemon paths share `mount_call` in `cmd/now.rs`, extracting hot's existing special error mapping. Unwatched errors become `CliError::Precondition`, whose current exit code is **5**. JSON emits the command's own `tidemark.<command>.v1` schema with `data.error`. Ordinary remote errors still use the existing mapping. Direct paths and all measurement/attribution/filtering logic are unchanged. No dependencies, manifests, modules, or out-of-scope paths changed.

## Reproduction and regression evidence

Before implementation, `cargo test --offline -p tidemarkd --lib rejects_unwatched_mount` ran four tests and all four failed: `now` accepted `/B`; the other methods lacked the required unwatched diagnostic. The transcript is in the attempt output `library/reproduction.log`.

- `phantom_rejects_unwatched_mount`, `writers_rejects_unwatched_mount`, `lvm_rejects_unwatched_mount`, `now_rejects_unwatched_mount`: assert exact code and message for `/B`, `/A/subdir`, and configured `/A` after an absent-mount publication, using injected time 120.
- `watched_lvm_mount_without_data_is_empty_success`: a completed empty LVM publication remains an empty successful response for watched `/A`.
- Existing `hot_unwatched_mount_is_distinct_from_watched_idle_mount` remains unchanged.
- `cmd::now::mount_filter_tests::phantom_unwatched_mapping_in_text_and_json`: socket-free CLI mapping test exercises all five commands, checks the human precondition message and exit code 5, and parses each JSON schema/error document.
- Existing integration test adaptations: **none**.

## Before/after output

For the reported guest command `tidemark phantom --mount /mnt/p12bind`, the supplied baseline observation was empty stdout and exit 0. With an API unwatched response, the CLI now exits 5 and reports:

```text
phantom: <socket>: /mnt/p12bind is not watched (watch.mounts = [<configured mounts>]); add it to [watch].mounts and run: tidemark deploy stage --from <file>
```

Under `--json`, stdout uses `schema: "tidemark.phantom.v1"` and `data.error.code: "unavailable"`, with the daemon message prefixed `unavailable: `. The new unit test verifies the exact diagnostic for `/B` and configuration `["/A"]`; this worker did not run the guest command.

## Assumptions

- Preserve hot's current treatment of a configured mount marked absent, including its “not watched” wording, as explicitly requested.
- A socket-free test of the CLI mapper is the local human/JSON regression; the unchanged existing hot binary test remains the transport-level gate on socket-capable hosts.
- Use the precreated attempt branch; no branch creation, merge, push, or memory projection edits.
- Build offline with the host Cargo cache and pinned 1.98.0 toolchain. Set `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`.

## Validation

All commands use offline mode and `timeout 1500`. Full logs are in the attempt output `library/`.

| Command | Local outcome |
|---|---|
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --workspace --no-fail-fast` | Exit 0; 87 harness summaries total 1,021 passed / 0 failed / 14 ignored (includes nested child summaries; not a unique-test census) |
| `cargo build --workspace --features privileged --tests` | Exit 0; compile only, pending VM runtime |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Exit 0 |
| `tools/ci.sh` | Exit 0; 150 owner tags and forbidden-symbol scan passed; all available checks passed |

Focused daemon unit run: 133 passed / 0 failed. Focused CLI mapping run: 1 passed / 0 failed. Six new tests total: four rejection regressions, one watched empty-success regression, and one CLI mapping regression (ten method/output combinations). Root socket restrictions can cause existing socket-dependent tests to return early; no socket-capable run is claimed here.

Cargo-deny is not installed: the CI wrapper explicitly skipped its audit. Coordinator should run the dependency audit on an equipped host. No new dependency was introduced.

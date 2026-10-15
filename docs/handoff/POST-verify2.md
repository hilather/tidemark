# POST-verify2 handoff

## Needs from coordinator

Independent review and the uid 1000/umask 022/socket and Ubuntu runner gates remain pending. Existing privileged deployment/reboot tests remain pending VM; this attempt only builds them. This document is an evidence candidate, not release approval.

## Rule chosen

Without `--wait`, verification retries every 500 ms within a ten-second monotonic budget only when the sampler is exactly `degraded` with detail `measurement pass in progress`, every other core component appears exactly once and is `ok`, and service/config/API checks pass. The final verdict still requires every core component to be `ok`. Expiry retains the failing result naming `sampler=degraded`. Other degradation, missing/duplicate core entries, API failure or a mismatching hash does not earn grace.

With nonzero `--wait`, that duration owns the entire budget. The routine sampler state uses fast polling inside it; other failures retain five-second polling and the existing stopped-service early return. Grace never extends the wait. Socket connect and exchange each receive at most half the remaining budget (and no more than the configured socket timeout). A zero budget fails before opening a socket. Systemctl/file probes retain their existing behavior; this is not a hard real-time guarantee against host scheduling delays or slow service probes.

The internal Rust `ApiPing` adapter gained a default budget-aware hook for source compatibility with existing fake/ping-only implementations. `SocketPing` overrides it to bound both phases. No daemon, status protocol/API method, schema, JSON field, CLI flag, exit code, dependency or manifest changed. Operator documentation describes the grace.

## Regression evidence

`verify_sampler_grace_sequences` covers in-progress then healthy with zero wait, persistent in-progress expiry with the sampler named, another degradation detail failing after one call, another failed core component failing immediately, duplicate sampler entries failing immediately, ordinary degradation recovering under explicit wait, and explicit wait shorter than grace. Timing parameters reduce grace to 100 ms for zero-wait expiry and polling to 2 ms; explicit-wait cases retain a ten-second grace parameter so their short deadline proves it is not added. `verify_wait_rechecks_live_health` retains the existing ordinary-degradation recovery case using the short polling seam.

A negative-control run restored the previous zero-wait immediate-return policy in the candidate polling loop: `verify_sampler_grace_sequences` failed its first recovery verdict (`false` versus `true`), exit 101, 0 passed/1 failed. Restoring the candidate produced 8 focused verifier passes in 0.15 seconds. This isolates the old policy; it is not a claim that an unmodified baseline tree was rebuilt. The zero-budget socket test also passes without sockets.

## Assumptions

- The specific POST task supersedes generic wave selection; the related W4-B section and its deployment contracts were read, together with the review and prior handoff.
- Exact matching is intentional: unrelated sampler degradation must never be treated as routine progress.
- Nonzero `--wait` may retry routine progress for its full requested duration; ten seconds is the implicit default budget, not an extra allowance on top of explicit waits.
- Parameterized timing is sufficient for these metadata-only, in-memory status sequences. No new privileged test is needed for the CLI-only rule.
- No branches, merges, pushes, service operations or host configuration changes performed. No shared-file edits or new dependencies.
- Validation uses host Cargo/Rustup caches and offline mode; logs are in the attempt output `library/`.

## Validation

All long commands ran under `timeout 1500`, with offline Cargo and host caches. Commands and transcripts are retained in the attempt output `library/`.

- `cargo fmt --check`: passed; final restored-candidate rerun passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed, and CI repeated it after the final test edits.
- `cargo test --offline --workspace --no-fail-fast`: exit 0; 1,004 passed, 0 failed, 14 ignored across 87 suite summaries (including child harnesses). CLI library: 146 passed, 1 ignored.
- `cargo build --offline --workspace --features privileged --tests`: passed; privileged execution pending VM.
- `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`: passed.
- `tools/ci.sh` with `CARGO_NET_OFFLINE=true`: exit 0; source checks (150 owner tags), formatting, Clippy, strict-ancestry checks, workspace tests and privileged-feature build passed. Log totals: 1,006 passed, 0 failed, 14 ignored across 88 summaries, including strict-ancestry/child harnesses.
- CI warned that `cargo-deny` is not installed and skipped dependency-policy validation.
- Final focused verifier rerun: 8 passed, 0 failed, 139 filtered, 0.15 seconds. Old-policy negative control: expected exit 101, 0 passed, 1 failed.
- `git diff --check`: passed. All modifications are inside owned paths; no shared-file changes.
- Environment: uid 0, umask 022. No uid 1000, GitHub runner, real service/reboot or privileged runtime acceptance is claimed. Socket-denied skips and ignored tests remain limitations of local suite evidence.


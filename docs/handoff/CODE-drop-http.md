# Handoff — CODE-drop-http: Remove HTTP from v1

## Needs from coordinator

Run the five known readiness cases in unrestricted CI, dependency auditing on an equipped host, and the existing privileged daemon gates in disposable Rocky guests. Verify LVM reporting under the new unit restriction and absence of daemon TCP listeners. This is an implementation/evidence candidate; independent verification and integration remain pending.

Date: 2026-10-13. Attempt: `attempt-f36335ec68c2252c9af5cb74b3bbe3d637be4c635fa42828e27ba76eaf5695a2`. Branch: supplied attempt branch; no branch creation, merge or push.

## What changed

- Core: removed `MetricsConfig.listen`, `AlertsConfig.webhook`, defaults and loopback validation. Bounded config loading detects either obsolete key, including empty and wrongly typed values, before typed deserialization and names the key with `feature was removed in v1 (ADR-020)`. Default alert rules need no change. Updated the pinned example and replaced endpoint validation tests with load/byte-parser rejection regressions.
- Live: deleted `WebhookDeliver`, queue, retry/timeout logic, JSON/network payload construction, counters, shutdown thread and five transport-only tests. Kept `JournalDeliver`, structured fields and the `Deliver` seam; removed webhook-only error variants. The alert engine remains unchanged.
- Daemon: deleted TCP bind/respond code, HTTP connection workers and admission thread, HTTP exposition cache/health and six HTTP-only tests. Textfile rendering, snapshot ranking/reconciliation (including initial seeding), checked monotonic scheduling, atomic handle-relative publication, staging recovery, caps and hourly error logging remain. Journal delivery, persisted alert events, diagnostics and collector availability/continuity logic remain. No removed-key references existed in daemon lib/status/firstboot wiring.
- Packaging: removed keys from the reference config and marker-only upgrade copy; installed unit, normative unit and privileged systemd-run fixture now use `RestrictAddressFamilies=AF_UNIX AF_NETLINK`. Packaging READMEs document textfile/journal and Unix API behavior. Grafana and Prometheus rules contain no endpoint scraping configuration, so need no change.
- Plan: added ADR-020 with owner date 2026-10-12, rationale, removals and revisit condition; updated 08's configuration/unit/metrics contract and 07's delivery section. 08 has no separate reference-table entries for these keys beyond its removed validation prose. No unrelated plan cleanup performed.
- CLI: source/tests untouched, workspace build succeeds. A CLI binary regression check runs `tidemark --config <tempfile> status` with each obsolete key; both return exit 2 and the exact ADR-020 message (`library/cli-removed-keys.log`). CLI loads via core `Config::load`.

## Shared-file edits and dependencies

`Cargo.toml`: removed workspace `ureq` and `tiny_http`. `crates/tidemark-live/Cargo.toml`: removed runtime client and test server edges. `Cargo.lock`: Cargo pruned offline; no dependency added/downloaded. Daemon manifest had already lost tiny_http in EDGE-metrics, and deny.toml had no entries for either package. No lib.rs/module declarations changed.

67 versioned package entries left Cargo.lock, with no additions. 64 package names disappeared entirely (listed below); the other three removed old versions are `base64 0.23.1`, `getrandom 0.2.17`, `windows-sys 0.52.0`, whose names still exist at other versions. Exact versioned removals are in `library/removed-package-records.txt`.

64 names removed entirely:

ascii, chunked_transfer, cookie, cookie_store, deranged, displaydoc, document-features, form_urlencoded, http, httparse, httpdate, icu_collections, icu_locale_core, icu_normalizer, icu_normalizer_data, icu_properties, icu_properties_data, icu_provider, idna, idna_adapter, litemap, litrs, num-conv, percent-encoding, potential_utf, powerfmt, ring, rustls, rustls-pki-types, rustls-webpki, stable_deref_trait, subtle, synstructure, time, time-core, time-macros, tiny_http, tinystr, untrusted, ureq, ureq-proto, url, utf8-zero, utf8_iter, wasi, webpki-roots, windows-targets, windows_aarch64_gnullvm, windows_aarch64_msvc, windows_i686_gnu, windows_i686_gnullvm, windows_i686_msvc, windows_x86_64_gnu, windows_x86_64_gnullvm, windows_x86_64_msvc, writeable, yoke, yoke-derive, zerofrom, zerofrom-derive, zeroize, zerotrie, zerovec, zerovec-derive.

## Acceptance evidence

- `removed_http_keys_have_actionable_errors`: Config byte parser and file loader reject both keys for empty string, populated string and integer.
- `check_config_rejects_removed_http_keys`: actual daemon subprocess with `--check-config` rejects both with exit 2 and named ADR-020 message.
- `resolved_workspace_has_no_http_or_tls_packages`: runs `cargo metadata --offline --format-version 1` and checks all resolved workspace packages for ureq, tiny_http, rustls, ring, hyper, reqwest and webpki-roots.
- `unit_allows_only_unix_and_netlink` and `exact_normative_unit_and_config`: exact address families and pinned unit/config.
- Existing textfile/exporter, alert persistence/availability, core config, CLI/API suites retained. Existing privileged fixture compiles only; execution **pending VM**.

## Assumptions and scope

The explicit task authorizes these core contract removals and dependency deletions. The removal diagnostic applies to configuration loaded through Config::load/from_toml_bytes; direct generic serde deserialization still rejects obsolete fields as unknown. The bounded input is parsed as TOML values for presence checks, then through existing typed deserialization, preserving existing typed error details. No replacement flag, stub or network feature was added. Netlink is permitted for child LVM reporting tools; the daemon itself retains only its Unix API socket.

All work stayed in approved paths. No new dependency, external communication, privileged test execution, LVM/service/mount command or workstation system-directory write occurred. Memory receipts were empty on initial inspection. Read retained CORE-fix-1/2/3 and EDGE-metrics/API handoffs; preserved their non-HTTP behavior. Historical plan claims remain for the later plan task.

## Remaining workspace mentions

Outside docs/plan, docs/handoff and docs/reviews, the required sweep finds only intentional removal diagnostic checks in config.rs, core review_regressions.rs and daemon w6a1.rs; banned-package names in daemon w6b.rs; absence assertions in xtask/tests/w6c.rs; and ADR-020 removal explanations in the two packaging READMEs. No production HTTP implementation or manifest dependency remains. `.listen` in other code is Unix API socket handling, not an internet socket. No shell script changed.

## Remaining plan mentions

The exact requested grep is saved in `library/remaining-plan.txt`. Files retaining endpoint/webhook mentions:

- `docs/plan/01-requirements.md`
- `docs/plan/02-architecture.md`
- `docs/plan/08-interfaces.md`
- `docs/plan/12-decisions-and-risks.md`
- `docs/plan/13-contract-decisions.md`
- `docs/plan/waves/wave-5-live.md`
- `docs/plan/waves/wave-6-daemon-packaging.md`
- `docs/plan/waves/wave-7-hardening-rollout.md`

08 and ADR-020 mention only rejection/removal. Other historical architecture/requirements/wave/contract-decision and 12 risk entries are intentionally left for the later plan task.

## Guest commands (coordinator only)

On disposable Rocky guests with the installed unit:

```sh
systemd-analyze verify /usr/lib/systemd/system/tidemarkd.service
systemd-analyze security tidemarkd.service
systemctl show tidemarkd.service -p RestrictAddressFamilies
ss -ltnp
journalctl -u tidemarkd.service --no-pager
```

Confirm ss shows no tidemarkd listener; inspect Unix API availability separately. Run the existing `privileged::daemon_end_to_end` gate with `--features privileged --test w6a3` only in a VM. For actual LVM collection, configure lvm enabled and a short valid interval in the guest, restart the installed unit, and check successful LVM component/report via `tidemark status`/`tidemark now` and journal under `AF_UNIX AF_NETLINK`. If testing reporting tools separately, guest-only `systemd-run --wait --pipe --property=RestrictAddressFamilies="AF_UNIX AF_NETLINK" lvs --reportformat json` and the equivalent vgs reporting command. Compare collection without the property; verify thin-pool data/metadata percentages are available. The coordinator decides whether AF_UNIX alone suffices; this candidate keeps AF_NETLINK as instructed.

## Validation

Formatting, both Clippy variants, privileged test compilation and rustdoc pass. Workspace runtime tests have only the five documented readiness failures. CI final result is recorded below after completion. Logs are in this attempt's output library. All long commands use timeout 1500, cached Rust 1.98.0, host Cargo/Rustup homes, four build jobs and two test threads. No privileged test is run.

Test source declarations across tracked Rust files: baseline 712, candidate 704 (includes privileged/ignored declarations, not a fresh baseline execution). Removed five live and six daemon HTTP tests; added three daemon regressions; replaced one core test. Runtime summaries and environmental failures follow below.

| Command | Exit / evidence |
|---|---|
| `cargo fmt --check` | 0; fmt.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0; clippy.log |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0; clippy-privileged.log |
| `cargo test --offline --workspace --no-fail-fast` | 101; 642 passed, 5 failed, 13 ignored; test.log, test-counts.json |
| `cargo build --offline --workspace --features privileged --tests` | 0; compilation only; privileged-build.log |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0; doc.log |

Five failing tests: `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, `startup_reads_last_complete_from_catalog`. Each hits the readiness deadline in w6a1. These are the documented sandbox category; EDGE-api additionally records the unowned baseline fixture's private-ancestor mismatch, so unrestricted execution needs that owning-task correction before claiming acceptance. This attempt does not change unrelated readiness behavior. Existing early returns for socket/root restrictions are harness passes, not real operator/transport acceptance.

No fresh baseline runtime run was made. Baseline runtime inventory inferred from the eight-test net reduction would have eight more tests than this candidate; the exact baseline source inventory is recorded above. No new privileged test is necessary for removing transports; existing privileged unit fixture changed and compiled. Runtime VM gates, dependency audit and independent verification remain pending.

`CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh` exits 1 at the same five w6a1 readiness failures. Source policy reports 141 owner tags and a clean forbidden-symbol scan; fmt and Clippy pass. Later CI build/deny stages are not reached; the separate privileged build and rustdoc commands pass. `cargo deny --version` exits 101 because cargo-deny is not installed. No dependency audit is claimed.

## Definition of Done checkpoint

- Owned paths and approved core removals only; shared edits listed above; scope and diff whitespace checked.
- Format, both Clippy variants, offline resolution, privileged compilation and rustdoc pass.
- New acceptance regressions and CLI binary checks pass; textfile and retained alert tests pass.
- Workspace tests/CI remain non-green solely at the five documented readiness cases; unrestricted CI and VM acceptance are pending.
- Public retained delivery items are documented; no unsafe or production unwrap/expect added.
- Evidence/report written, focused commits retained, no merge/push. Cleanup and submission receipt are recorded in the external worker report.

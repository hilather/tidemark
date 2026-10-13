# Tidemark parser fuzzing

Owned by: W7-C3-fuzz. This standalone cargo-fuzz workspace leaves the parent workspace manifest and lockfile unchanged. Install a nightly Rust toolchain and `cargo install cargo-fuzz --locked`, then from the repository root run:

```sh
cargo +nightly fuzz build
cargo +nightly fuzz run mountinfo -- -max_total_time=600
```

Replace `mountinfo` with any target in `fuzz/Cargo.toml`. The nightly/manual GitHub workflow builds all targets and runs each target for ten minutes in a separate matrix job. Its 45-minute job deadline includes installation/build time. It caps input length at 1 MiB, RSS at 2 GiB, and individual executions at two seconds. Local longer runs can use `-max_total_time=86400`; ten-minute nightly runs alone do not establish the release requirement of 24 cumulative crash-free hours.

A crash produces an input under `fuzz/artifacts/<target>/`. Preserve it and the stderr panic/timeout/OOM message, reproduce with `cargo +nightly fuzz run <target> <artifact>`, and minimize with `cargo +nightly fuzz tmin <target> <artifact>`. Add a named regression smoke test; if it demonstrates a product panic, mark it `#[ignore = "bug: <description>"]` and request a separately owned fix. Do not patch product parsers in this task. Retain any proptest regression files beside their smoke tests.

Smoke tests run offline on stable:

```sh
cargo test --offline -p tidemark-fs --test fuzz_smoke
cargo test --offline -p tidemarkd --test fuzz_smoke
cargo test --offline -p tidemark-cli --test fuzz_smoke
cargo test --offline -p tidemark-store --test fuzz_smoke
```

The fs, CLI and store suites use 48 cases per property and seed `0x57374333`; they include arbitrary bytes and truncation, flips, duplication, long fields, NUL/invalid UTF-8 mutations. Large inputs have two-second debug elapsed assertions. Deep JSON/TOML-shaped arrays exercise deserializer nesting rejection. Core configuration properties reside in CLI because core has no proptest dependency. Daemon report tests enumerate deterministic mutations because no permitted proptest-bearing crate depends on tidemarkd.

Corpus `sample` files for mountinfo, procfs, config and LVM are copied from recorded-format test fixtures or the shipped config. Other seeds mirror existing inline API/deployment/catalog samples; they are synthetic, not host captures. Support modules are compiled by the smoke suites and reused by corresponding fuzz targets. They write only disposable temporary state and use inert service/API boundaries.

Coverage limitations: API targets deserialize the exact public Request/Response types used by daemon/client. The response target also reaches public Client::call via a disposable local listener and checks framing/envelope behavior without dispatch. Stable socket properties return early on listener EPERM and print the exclusion in their bounds test; GitHub fuzzing asserts listener availability. Daemon request socket framing and dispatch remain uncovered. Fanotify's private decoder owns descriptors extracted from its buffer; `EventIter` has no public constructor and arbitrary byte injection is unsafe. No fake target is supplied. The fdinfo mount-id parser is private in tidemark-live and cannot be fed arbitrary text through its public watcher method. See the handoff for coordinator follow-ups.

The cargo-fuzz manifest, targets and workflow are unverified until their first nightly GitHub run. Do not build this project offline in the worker environment: nightly and libfuzzer-sys are unavailable there.

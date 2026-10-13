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
cargo test --offline -p tidemark-sys --test fuzz_smoke
cargo test --offline -p tidemark-live --test fuzz_smoke
cargo test --offline -p tidemarkd --lib api::tests::frame_
```

The fs, CLI and store suites use 48 cases per property and seed `0x57374333`; they include arbitrary bytes and truncation, flips, duplication, long fields, NUL/invalid UTF-8 mutations. Large inputs have generous 30-second debug elapsed guards plus coarse scaling checks. Deep JSON/TOML-shaped arrays exercise deserializer nesting rejection. Core configuration properties reside in CLI because core has no proptest dependency. Daemon report tests enumerate deterministic mutations because no permitted proptest-bearing crate depends on tidemarkd.

Corpus `sample` files for mountinfo, procfs, config and LVM are copied from recorded-format test fixtures or the shipped config. Other seeds mirror existing inline API/deployment/catalog samples; they are synthetic, not host captures. Support modules are compiled by the smoke suites and reused by corresponding fuzz targets. They write only disposable temporary state and use inert service/API boundaries.

The FUZZ-seams additions expose the same entry points to stable generated tests and nightly targets:

| Target | Entry point | Stable coverage | Seeds |
|---|---|---|---|
| `fanotify_events` | `tidemark_sys::fanotify::parse_event_buffer` | sys `tests/fuzz_smoke.rs`; existing owning-iterator unit tests | native-endian fd metadata, FID, DFID_NAME |
| `mount_id` | `tidemark_live::fuzzing::parse_mount_id` | live `tests/fuzz_smoke.rs` | fdinfo with mount 42 |
| `api_frame` | `tidemarkd::api::read_frame` + `FrameDecoder` | daemon `api.rs` source-local frame tests | newline-terminated status request |

These three owner crates have no proptest dependency. With dependency additions prohibited, their stable suites use a shared deterministic xorshift byte generator (260 lengths) and exhaustive seed truncations and four byte flips per position. They also assert valid decoding, typed refusals, bounded progress, generous timing, and scaling. This is generated property-style coverage without proptest shrinking; exact proptest coverage remains a coordinator follow-up. Mount text uses UTF-8 lossy conversion, matching the procfs caller boundary. Fanotify records are lazy and non-owning, with no record allocation or descriptor side effects. Framing capacity never exceeds one MiB (including the newline); normal serde Request allocations remain proportional to the bounded frame, not an assertion that total deserialization memory fits one MiB.

Coverage limitations: response transport properties still require an unrestricted Unix listener. Request framing now covers the production pure seam, but the new target does not dispatch methods or exercise peer checks or clocks. Those remain covered by existing connection/socketpair and guest tests. The daemon protocol is JSON lines, not length-prefixed; its frozen wire format is unchanged. Fanotify seeds use native-endian Linux metadata (CI's x86_64 convention).

The original 13 targets built and ran ten minutes each on GitHub per the task brief. The three new targets still need their first nightly GitHub build/run. Do not build this project offline in the worker environment: nightly and libfuzzer-sys are unavailable there.

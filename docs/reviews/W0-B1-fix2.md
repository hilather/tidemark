# W0-B1-fix2 — Recheck fixes

R1: `subject_suffix` now preserves only `/` and ASCII `[A-Za-z0-9._-]`, encoding every other byte as uppercase `%XX`. Both the helper and public default-rule rustdoc describe this format. No merge policy changed.

`percent_encoded_default_names_and_disabled_overrides` checks colon, percent, space, backslash, UTF-8 and the preserved ASCII characters. For each subject, a disabled same-name user rule remains the sole rule with that name after two merges, and validation succeeds. `stable_default_names_and_scheduled_roots` retains order independence, exact `/data` base names and scheduled-root coverage; its invalid-UTF-8/backslash expectations now use percent encoding.

| Subject | Generated name |
|---|---|
| `/srv:blue` | `fs_full_soon:/srv%3Ablue` |
| `/x%y` | `fs_full_soon:/x%25y` |
| `/x y` | `fs_full_soon:/x%20y` |
| `/données` | `fs_full_soon:/donn%C3%A9es` |

R2: refreshed `tests/example.toml` with the plan's throttle comment (`or a user-defined profile`). `example_verbatim_and_roundtrip` reads `../../docs/plan/08-interfaces.md` relative to `CARGO_MANIFEST_DIR`, isolates section 3's TOML block and compares its bytes, including the final newline, against the fixture before parsing and round-tripping it.

Validation uses Rust 1.98.0, a temporary copy of the host registry cache and offline mode. All five required commands exited 0: `cargo fmt --check`, `cargo clippy --offline --workspace --all-targets -- -D warnings`, `cargo test --offline --workspace`, `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`, and `sh tools/ci.sh` (with `CARGO_NET_OFFLINE=true`). Core ran 10 unit tests, 5 config-load tests and 12 review regressions. CI reported all available checks passed; cargo-deny was absent and its audit was skipped by the existing wrapper. No dependencies, shared manifests, lockfile, other crates or plan documents changed.

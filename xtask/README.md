# Workspace tooling

Run `cargo xtask ci` to validate owner tags and read-only symbols, check formatting,
run Clippy and unprivileged tests, and run cargo-deny when installed. Missing
cargo-deny produces a warning. The checks do not rewrite tracked files.

`tools/ci.sh` runs the same checks from the repository root. Set
`RUST_TOOLCHAIN_BIN` to the directory containing the pinned Cargo and Rust tools
when they are not on PATH.

`vm-test` is reserved for W0-C, `gen-fixture` and `bench` for W7-B, and `rpm` for
W6-C. These stubs exit with an error naming their owner.

The source scan parses Rust syntax, excludes `#[cfg(test)]` items and the
`test-support` crate and `tests` directories, and ignores comments and string
literals except command names passed directly to `Command::new`. It checks the
specified symbols, including macro arguments. It is a source-policy check,
not whole-program alias or data-flow analysis.

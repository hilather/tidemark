# CI-fix-w2a handoff

Out-of-scope blockers: `crates/tidemark-cli/tests/w2c/main.rs:807:34` and `:813:40` fail with E0277 (full build diagnostics below). An additional scan-library test fails at `crates/tidemark-scan/src/orchestrator.rs:1766:13`: `cancellation_during_publication_is_recorded_and_never_kept`, assertion left `"running"`, right `"cancelled"`. These files were not edited.

Updated the W2-A fixture walker with an explicit 64 MiB per-directory record bound; the small link-semantics fixture stays below it. Renamed its generic annotation label to `app.label`. No other constructors needed changes; no shared files or dependencies changed.

Offline validation used Rust 1.98.0, host `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`. `cargo fmt --check` passed. Both requested workspace builds, workspace Clippy and workspace tests exited 101 on the CLI errors; `tools/ci.sh` exited 1 on the same Clippy errors after source checks passed (134 owner tags). `cargo build -p tidemark-scan --features privileged --tests --offline` passed; privileged runtime tests remain pending VM. Additional `cargo test -p tidemark-scan --offline`: 34 library tests passed, one failed, two scale tests ignored; `cargo test -p tidemark-scan --test w2a --test w2b --offline` passed: W2-A 15 passed / 2 scale ignored, W2-B 21 passed.

Full out-of-scope build diagnostics:

```text
error[E0277]: `ScanEntry` doesn't implement `Debug`
   --> crates/tidemark-cli/tests/w2c/main.rs:807:34
    |
807 |     let before = format!("{:?}", catalog.list(Some(&root)).unwrap());
    |                           ----   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ScanEntry` cannot be formatted using `{:?}` because it doesn't implement `Debug`
    |                           |
    |                           required by this formatting parameter
    |
    = help: the trait `Debug` is not implemented for `ScanEntry`
help: the trait `Debug` is conditionally implemented for `Vec<T, A>`
   --> /rustc/88d9e12ae178fab0fb5cc050a94da85685d449ea/library/alloc/src/vec/mod.rs:4321:8
    |
    = note: unsatisfied requirement introduced here: `ScanEntry: Debug`
    = note: required for `Vec<ScanEntry>` to implement `Debug`

error[E0277]: `ScanEntry` doesn't implement `Debug`
   --> crates/tidemark-cli/tests/w2c/main.rs:813:40
    |
813 |     assert_eq!(before, format!("{:?}", catalog.list(Some(&root)).unwrap()));
    |                                 ----   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ScanEntry` cannot be formatted using `{:?}` because it doesn't implement `Debug`
    |                                 |
    |                                 required by this formatting parameter
    |
    = help: the trait `Debug` is not implemented for `ScanEntry`
help: the trait `Debug` is conditionally implemented for `Vec<T, A>`
   --> /rustc/88d9e12ae178fab0fb5cc050a94da85685d449ea/library/alloc/src/vec/mod.rs:4321:8
    |
    = note: unsatisfied requirement introduced here: `ScanEntry: Debug`
    = note: required for `Vec<ScanEntry>` to implement `Debug`

For more information about this error, try `rustc --explain E0277`.
error: could not compile `tidemark-cli` (test "w2c") due to 2 previous errors
warning: build failed, waiting for other jobs to finish...
```

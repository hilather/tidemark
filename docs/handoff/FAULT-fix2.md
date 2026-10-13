# Handoff — FAULT-fix2: use guest tools in fault scripts

- Branch: precreated attempt branch `hp-f861bdbd448fa29053b7c7b8842a144e6a46fb56ffb61410eeb33f78cd54d75c`.
- Date: 2026-10-13. Status: implemented evidence candidate; guest validation pending VM.

## Changes

Replaced the suite's only ripgrep invocation with `grep -Fq NoSpace`, preserving the accepted exit codes and literal stderr requirement. Audited all six entrypoints, common.bash, and both Python helpers. Added a shared `mkfs.xfs` prerequisite check before temporary-directory allocation or resource setup; missing xfsprogs produces an explicit FAIL, installation guidance, and nonzero exit. Existing executable checks cover the locally built product binaries. README now describes the prerequisite check and regression guard.

The new w7c check recursively scans `.sh`, `.bash`, and `.py` sources for unsupported command words (`rg`, `jq`, `fd`, `bc`). Its conservative lexical guard also catches quoted Python subprocess argv, absolute command paths, inline command strings, and calls through run_helper. Unit fixtures cover each forbidden tool in those forms. Full-line comments are excluded; ordinary strings are deliberately included. Computed/obfuscated commands are not analyzed, and unrelated uses of these exact words in strings can produce false positives.

## External-command audit

| Origin | Commands / coverage |
| --- | --- |
| Bash builtins | `cd`, `command`, `echo`, `exec`, `exit`, `kill`, `local`, `printf`, `pwd`, `read`, `set`, `shift`, `source`, `trap`, `umask`, `wait`; conditionals/arithmetic are Bash syntax |
| coreutils | `basename`, `cat`, `chmod`, `cp`, `dirname`, `ls`, `mkdir`, `mktemp`, `rm`, `sleep`, `timeout`, `truncate` |
| grep | `grep -Fq` for the literal NoSpace check |
| util-linux | `findmnt`, `losetup`, `mount`, `setsid`, `umount` |
| python3 | Standard-library-only helpers and heredocs; neither helper invokes subprocesses; SQLite module remains an existing documented prerequisite |
| Extra dependency | `mkfs.xfs` from xfsprogs: now checked at the beginning of shared setup, before resources |
| Product executables | `target/debug/tidemark`, `target/debug/tidemarkd`: existing executable checks before resources |

README workstation/guest preparation examples use rsync, ssh, cargo, and sudo as transport/build commands, rather than commands executed by the fault scripts.

## Validation

Validation uses pinned Rust 1.98.0 on PATH, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_HOME=/tmp/tidemark-fault2-cargo` (temporary copy of the available registry cache). No dependency or shared manifest/module changes.

- `bash -n` on all six `.sh` entrypoints and `common.bash`: passed.
- Python AST parse of both helpers: passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test -p xtask --offline`: passed, 36 tests (18 unit, 9 w6c, 6 w7b, 3 w7c).
- `cargo clippy -p xtask --all-targets -- -D warnings` with `CARGO_NET_OFFLINE=true`: passed.
- Shellcheck is absent locally. The existing w7c test invokes `shellcheck -x` at default severity when available; equipped GitHub validation remains required. Reviewed the new shell check for quoting and builtin usage.
- No fault script or privileged command was executed. Existing guest scenarios remain **pending VM**; this portability repair does not claim guest acceptance or repair the separate snapshot-cleanup product fault.

## Scope and deviations

Only `tests/fault/**`, `xtask/tests/w7c.rs`, and this handoff changed. No shared-file edits, new dependencies, contract changes, branch creation, merges, or pushes. No memory updates were pending in the receipts checked at start. Narrow offline xtask validation follows the explicit assigned-task instructions; full workspace CI and VM execution were not requested for this attempt.

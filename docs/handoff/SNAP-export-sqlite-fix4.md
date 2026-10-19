# SNAP-export-sqlite-fix4

## Needs from coordinator
Nothing.

## Changes
- `export_sqlite.rs`: comment at the classification stating the rule (SQLite I/O error is exit 5 only when the pre-cleanup probe confirms ENOSPC/EDQUOT, else exit 1) and why (no safe errno access; `sqlite3_system_errno` is unsafe).
- `tests/export_sqlite.rs`: renamed the s1 test; control comment "SQLite I/O error not confirmed as space → exit 1 (by design)"; persistent-quota assertion (exit 5) kept.
- `docs/operator-guide.md`: one sentence on exit 5 vs. unconfirmed transient quota error (exit 1).
- No behaviour change.

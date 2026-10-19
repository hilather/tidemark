# SNAP-export-ownertag handoff

Changed the first-line owner tag from `//! Owned by: SNAP-export-sqlite` to `//! Owned by: WSNAP-export-sqlite` in `crates/tidemark-store/src/export_sqlite.rs`, `crates/tidemark-store/tests/export_sqlite.rs` and `crates/tidemark-cli/tests/snapshots_export.rs`, so the `xtask ci` owner-tag check (`//! Owned by: W`) passes. No other changes; no dependencies or shared files touched.

//! Owned by: W7-C3-fuzz
//! Reach stored-path resolution using a temporary catalog and raw SQL fixture.
// Fixture setup failures should fail the test or fuzz execution immediately.
#![allow(clippy::unwrap_used)]
use std::{ffi::OsString, os::unix::{ffi::OsStringExt, fs::PermissionsExt}, path::PathBuf};
use tidemark_store::catalog::{Catalog, ScanEntry};
/// Feed adversarial bytes through the public parser seam in disposable state.
pub fn exercise(bytes: &[u8]) {
    let dir = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir().unwrap();
    let catalog = Catalog::open(dir.path(), true).unwrap();
    let entry = ScanEntry {
        scan_id: "fuzz".into(),
        root: tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap(),
        mode: tidemark_core::scan::ScanMode::Walk,
        started_at: 0,
        finished_at: None,
        status: "complete".into(),
        degraded: false,
        file: Some(
            dir.path()
                .join(PathBuf::from(OsString::from_vec(bytes.to_vec()))),
        ),
        t_disk: None,
        t_inodes: None,
        t_dirs: None,
        errors: None,
        duration_ms: None,
    };
    let _ = catalog.record_finish(&entry);
    // Direct SQL supplies legacy and corrupt stored paths too, bypassing writer normalization.
    let conn = rusqlite::Connection::open(dir.path().join("timeseries.db")).unwrap();
    struct Bytes<'a>(&'a [u8]);
    impl rusqlite::types::ToSql for Bytes<'_> {
        fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
            Ok(rusqlite::types::ToSqlOutput::Borrowed(
                rusqlite::types::ValueRef::Text(self.0),
            ))
        }
    }
    // Ensure a row exists even when the writer refused the candidate path.
    let mut valid = entry;
    valid.file = None;
    catalog.record_finish(&valid).unwrap();
    conn.execute(
        "UPDATE scans SET file=? WHERE scan_id='fuzz'",
        [Bytes(bytes)],
    )
    .unwrap();
    let _: Result<_, tidemark_store::StoreError> = catalog.get("fuzz");
}

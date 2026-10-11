# SCR-1: Explicit corrupt time-series recovery entry point

- Raised by: W1-E    Date: 2026-10-11
- Affects: store API recovery contract    Docs: 05-storage.md §4, §6, §9
- Affected tasks: W1-E, W2-C, W5-D

## Problem

The catalog shares timeseries.db with samples. Catalog::open(state_dir, rw) must return an error when SQLite reports a corrupt database. Consequently, Catalog::rebuild_from_files(&self) cannot be called in the corrupt-database fallback described in §4. Automatically replacing the sample database on every writable open would discard unrelated samples without an explicit recovery request.

The implementation can rebuild an absent database and atomically replace scan rows in a healthy database. Corrupt-database recovery is not implemented.

## Proposal

Add an explicit recovery entry point:

    pub fn recover_catalog(state_dir: &Path) -> Result<Catalog, StoreError>;

The caller must first stop the writer and close all sample/catalog connections. Recovery preserves timeseries.db and any WAL/SHM sidecars under unique recovery names, creates a new v1 database, then rebuilds catalog rows from valid published snapshots. Report that historical samples were preserved in the recovery files but are unavailable in the new database. Keep ordinary Catalog::open behavior unchanged.

## Impact if accepted

Coordinator attaches the declaration and assigns recovery implementation/tests. The snapshots and SQL schemas do not change. CLI rebuild-catalog can invoke this explicit entry point after ordinary open fails.

## Interim

Catalog::open returns the SQLite error for corruption. Missing-database initialization and healthy-catalog rebuilding work; no corrupt file or sample history is overwritten.

## Decision (coordinator)

Pending.

# SCR-1: Allow snapshot SQL access for cross-snapshot queries

- Raised by: W3-A    Date: 2026-10-11
- Affects: Snapshot reader interface; Docs: 05-storage §5.1–5.2
- Affected tasks: W3-A; potentially W3-B and W3-C

## Problem
Snapshot owns a private `conn: Connection` and stores no source path. There is no connection accessor. The W3-A frozen input is `&Snapshot`, so its owned diff.rs cannot ATTACH the old snapshot and execute the normative joins. reader.rs is outside this attempt's write scope.

## Proposal
Add a crate-private accessor to reader.rs: `pub(crate) fn connection(&self) -> &rusqlite::Connection`. W3-A can then ATTACH a percent-encoded immutable/read-only URI with a bound parameter, execute the joins, and detach using a guard. An alternative is exposing the source path and opening a separate immutable connection.

## Impact if accepted
Coordinator edits reader.rs and assigns follow-up W3-A implementation work. No schema, core, or public API change is necessary.

## Interim
Uses bounded existing reader queries and raw-byte joins by exact path. Functional behavior is implemented, but the SQL/performance acceptance remains unresolved. No unauthorized reader edit was made.

## Decision (coordinator)
Pending.

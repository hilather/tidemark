//! Owned by: W2-B
//! Resolve notable representatives with one gated read per directory.
use super::BulkError;
use std::sync::atomic::{AtomicBool, Ordering};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::{FileKind, Gate, Ino},
};
type Names = Vec<(RawPath, Ino, RawName)>;
pub(super) fn resolve(
    mut needed: Vec<(RawPath, Ino)>,
    gate: &dyn Gate,
    cancel: &AtomicBool,
) -> Result<(Names, u64), BulkError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(BulkError::Cancelled);
    }
    if needed.len() > 1_000_000 {
        return Err(BulkError::Limit("one million notable names"));
    }
    needed.sort_unstable();
    needed.dedup();
    let mut names = Vec::new();
    let mut missing = 0;
    let mut pos = 0;
    while pos < needed.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(BulkError::Cancelled);
        }
        let path = &needed[pos].0;
        let end = pos + needed[pos..].partition_point(|(p, _)| p == path);
        let mut found = vec![false; end - pos];
        match tidemark_fs::walk::read_dir_entries(path, gate) {
            Ok(entries) => {
                for (name, ino, kind) in entries {
                    if kind == FileKind::Dir {
                        continue;
                    }
                    if let Ok(index) = needed[pos..end].binary_search_by_key(&ino, |(_, i)| *i) {
                        if !found[index] {
                            // read_dir_entries is sorted by raw name; first match is the representative.
                            names.push((path.clone(), ino, name));
                            found[index] = true;
                        }
                    }
                }
            }
            Err(e) if matches!(e.raw_os_error(), Some(2 | 20 | 40)) => (),
            Err(e) => return Err(BulkError::Io(e)),
        }
        missing += found.iter().filter(|v| !**v).count() as u64;
        pos = end;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(BulkError::Cancelled);
    }
    Ok((names, missing))
}

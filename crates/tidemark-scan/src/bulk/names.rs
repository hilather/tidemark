//! Owned by: W2-B
//! Resolve notable representatives with one gated read per directory.
use super::BulkError;
use std::sync::atomic::{AtomicBool, Ordering};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::{DevId, DirId, FileKind, Gate, Ino, NsRecord},
};
// Compact phase-1 identity index, sorted by (parent, name); no full paths retained.
pub(super) struct Identity {
    pub(super) id: DirId,
    pub(super) parent: Option<DirId>,
    pub(super) name: Option<RawName>,
    pub(super) ino: Ino,
    pub(super) dev: DevId,
}
impl Identity {
    pub(super) fn from_ns(ns: &NsRecord) -> Self {
        Self {
            id: ns.id,
            parent: ns.parent,
            name: ns.name.clone(),
            ino: ns.ino,
            dev: ns.dev,
        }
    }
}
pub(super) fn sort_identities(identities: &mut [Identity]) {
    identities.sort_unstable_by(|a, b| (a.parent, &a.name).cmp(&(b.parent, &b.name)));
}
fn lookup<'a>(root: &RawPath, path: &RawPath, identities: &'a [Identity]) -> Option<&'a Identity> {
    if !path.starts_with(root) {
        return None;
    }
    let mut current = identities.first().filter(|i| i.parent.is_none())?;
    for part in path.as_bytes()[root.as_bytes().len()..]
        .split(|b| *b == b'/')
        .filter(|p| !p.is_empty())
    {
        let index = identities
            .binary_search_by(|i| {
                i.parent
                    .cmp(&Some(current.id))
                    .then_with(|| i.name.as_ref().map(|n| n.as_bytes()).cmp(&Some(part)))
            })
            .ok()?;
        current = &identities[index];
    }
    Some(current)
}
type Names = Vec<(RawPath, Ino, RawName)>;
pub(super) fn resolve(
    mut needed: Vec<(RawPath, Ino)>,
    cfg: &tidemark_fs::walk::WalkConfig,
    identities: &[Identity],
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
        let identity = lookup(&cfg.root, path, identities);
        let entries = match identity {
            Some(identity) => tidemark_fs::walk::read_dir_entries_checked(
                &cfg.root,
                identity.dev,
                path,
                Some(identity.ino),
                gate,
            ),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::CrossesDevices,
                "unobserved directory",
            )),
        };
        if let Ok(entries) = entries {
            for (name, ino, kind) in entries {
                let full = path.join(&name);
                if kind == FileKind::Dir
                    || cfg.exclude.is_match(full.as_path())
                    || cfg.skip_mounts.iter().any(|p| full.starts_with(p))
                {
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
        missing += found.iter().filter(|v| !**v).count() as u64;
        pos = end;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(BulkError::Cancelled);
    }
    Ok((names, missing))
}

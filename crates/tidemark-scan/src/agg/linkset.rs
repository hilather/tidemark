//! Owned by: W2-A
//! Classify link groups and stamp their union contributions without recursion.
use super::*;
type Resolved = (Vec<LinkSetRow>, Vec<LinkInstanceRow>, Vec<[u64; 4]>);
pub(super) fn resolve(
    nodes: &mut [Option<Box<Node>>],
    mut sets: Vec<LinkAcc>,
    ids: &[u32],
    cfg: &AggConfig,
    candidates: &mut notable::Candidates,
) -> Result<Resolved, AggError> {
    // Scratch exists only for link-bearing scans and is released before output.
    let mut unions = if sets.is_empty() {
        Vec::new()
    } else {
        vec![[0u64; 4]; nodes.len()]
    };
    sets.sort_by_key(|s| s.attrs.ino);
    if sets.windows(2).any(|p| p[0].attrs.ino == p[1].attrs.ino) {
        return Err(AggError::Tree);
    }
    let mut rows = Vec::with_capacity(sets.len());
    let mut instances = Vec::new();
    for (seq, mut set) in sets.into_iter().enumerate() {
        let stamp = u64::try_from(seq)
            .map_err(|_| AggError::Overflow)?
            .checked_add(1)
            .ok_or(AggError::Overflow)?;
        let found = count(set.instances.len())?;
        let a = set.attrs;
        if set
            .instances
            .iter()
            .any(|(d, _)| ids.get(d.0 as usize).is_none_or(|&id| id == NONE))
        {
            return Err(AggError::Tree);
        }
        let complete = found >= a.nlink;
        set.instances.sort_by(|a, b| {
            ids[a.0 .0 as usize]
                .cmp(&ids[b.0 .0 as usize])
                .then_with(|| {
                    a.1.as_ref()
                        .map(RawName::as_bytes)
                        .cmp(&b.1.as_ref().map(RawName::as_bytes))
                })
        });
        let (rep, name) = set.instances.first().ok_or(AggError::Tree)?.clone();
        if ids[rep.0 as usize] == NONE {
            return Err(AggError::Tree);
        }
        let mut ancestor = rep.0;
        for (dir, _) in &set.instances {
            ancestor = lca::lca(nodes, ancestor, dir.0)?;
        }
        let bucket = tidemark_core::time::age_bucket(cfg.scan_start, a.mtime);
        let n = nodes[ancestor as usize].as_mut().ok_or(AggError::Tree)?;
        add(&mut n.own_apparent, a.apparent)?;
        add(&mut n.totals.charged, a.disk)?;
        add(&mut n.totals.age[bucket], a.disk)?;
        if complete {
            add(&mut n.totals.reclaim, a.disk)?;
        }
        if found == 1 {
            add(&mut n.totals.disk, a.disk)?;
            add(&mut n.totals.apparent, a.apparent)?;
            add(&mut n.totals.inodes, 1)?;
        }
        for (dir, name) in &set.instances {
            let n = nodes
                .get_mut(dir.0 as usize)
                .and_then(Option::as_mut)
                .ok_or(AggError::Tree)?;
            add(&mut n.totals.files, 1)?;
            if a.kind == tidemark_core::scan::FileKind::Symlink {
                add(&mut n.symlinks, 1)?;
            }
            n.totals.newest = n.totals.newest.max(a.mtime);
            if cfg.link_instances {
                instances.push(LinkInstanceRow {
                    ino: a.ino,
                    dir_id: ids[dir.0 as usize],
                    name: name.clone(),
                });
            }
            if found >= 2 {
                let mut v = dir.0;
                while v != NONE {
                    let n = nodes[v as usize].as_mut().ok_or(AggError::Tree)?;
                    let union = &mut unions[v as usize];
                    if union[3] == stamp {
                        break;
                    }
                    union[3] = stamp;
                    add(&mut union[0], a.disk)?;
                    add(&mut union[1], a.apparent)?;
                    add(&mut union[2], 1)?;
                    v = n.parent;
                }
            }
        }
        candidates.insert(
            FileRow {
                id: 0,
                dir_id: rep.0,
                name,
                ino: a.ino,
                disk: a.disk,
                apparent: a.apparent,
                mtime: a.mtime,
                nlink: a.nlink,
            },
            cfg,
        );
        rows.push(LinkSetRow {
            ino: a.ino,
            nlink: a.nlink,
            found,
            complete,
            disk: a.disk,
            apparent: a.apparent,
            mtime: a.mtime,
            lca_dir_id: ids[ancestor as usize],
        });
    }
    Ok((rows, instances, unions))
}

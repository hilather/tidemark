//! Owned by: W2-A
//! Iterative tree validation, preorder numbering and additive rollups.
use super::*;
pub(super) fn prepare(mut agg: Aggregator) -> Result<Prepared, AggError> {
    if agg.failed
        || agg.cfg.max_notable == 0
        || count(agg.cfg.max_notable).is_err()
        || agg.cfg.max_stored_errors > MAX_ITEMS
    {
        return Err(AggError::Capacity);
    }
    for g in &agg.gone {
        // Every Gone record represents a queued directory that changed after
        // discovery, including a filesystem change (W1-B counts each as a race).
        add(&mut agg.races, 1)?;
        if g.reason == tidemark_core::scan::GoneReason::OtherFs {
            agg.skipped.push(SkipRow {
                dir_id: g.parent.0,
                name: g.name.clone(),
                reason: tidemark_core::scan::SkipReason::OtherFs,
            });
        }
    }
    if agg
        .nodes
        .first()
        .and_then(Option::as_ref)
        .is_none_or(|n| n.parent != NONE)
    {
        return Err(AggError::Tree);
    }
    for id in 1..agg.nodes.len() {
        if let Some(n) = &agg.nodes[id] {
            let p = n.parent;
            let parent = agg
                .nodes
                .get_mut(p as usize)
                .and_then(Option::as_mut)
                .ok_or(AggError::Tree)?;
            parent.children.push(count(id)?);
        }
    }
    // Move child vectors out temporarily so name sorting can borrow all nodes.
    for id in 0..agg.nodes.len() {
        let Some(n) = &mut agg.nodes[id] else {
            continue;
        };
        let mut children = std::mem::take(&mut n.children);
        children.sort_by(|a, b| {
            agg.nodes[*a as usize]
                .as_ref()
                .map(|n| &n.name)
                .cmp(&agg.nodes[*b as usize].as_ref().map(|n| &n.name))
        });
        for pair in children.windows(2) {
            if agg.nodes[pair[0] as usize].as_ref().map(|n| &n.name)
                == agg.nodes[pair[1] as usize].as_ref().map(|n| &n.name)
            {
                return Err(AggError::Tree);
            }
        }
        if let Some(n) = &mut agg.nodes[id] {
            n.children = children;
        }
    }
    let mut order = Vec::new();
    let mut stack = vec![(0u32, 0u32)];
    let mut ids = vec![NONE; agg.nodes.len()];
    while let Some((id, depth)) = stack.pop() {
        if ids[id as usize] != NONE {
            return Err(AggError::Tree);
        }
        ids[id as usize] = count(order.len())?;
        order.push(id);
        let n = agg.nodes[id as usize].as_mut().ok_or(AggError::Tree)?;
        n.depth = depth;
        for &child in n.children.iter().rev() {
            stack.push((child, depth.checked_add(1).ok_or(AggError::Overflow)?));
        }
    }
    if order.len() != agg.nodes.iter().filter(|n| n.is_some()).count() {
        return Err(AggError::Tree);
    }
    let mut sets: Vec<_> = agg.links.into_values().collect();
    sets.append(&mut agg.bulk_links);
    let (links, instances, unions) =
        linkset::resolve(&mut agg.nodes, sets, &ids, &agg.cfg, &mut agg.candidates)?;
    let mut ends = ids.clone();
    for &id in order.iter().rev() {
        let n = agg.nodes[id as usize].as_ref().ok_or(AggError::Tree)?;
        let p = n.parent;
        let t = n.totals.clone();
        if p != NONE {
            ends[p as usize] = ends[p as usize].max(ends[id as usize]);
            let n = agg.nodes[p as usize].as_mut().ok_or(AggError::Tree)?;
            let pt = &mut n.totals;
            add(&mut pt.disk, t.disk)?;
            add(&mut pt.apparent, t.apparent)?;
            add(&mut pt.charged, t.charged)?;
            add(&mut pt.reclaim, t.reclaim)?;
            add(&mut pt.files, t.files)?;
            add(&mut pt.inodes, t.inodes)?;
            add(&mut pt.errors, t.errors)?;
            for (a, b) in pt.age.iter_mut().zip(t.age) {
                add(a, b)?;
            }
            pt.newest = pt.newest.max(t.newest);
            pt.max_entries = pt.max_entries.max(t.max_entries);
        }
    }
    for (id, slot) in agg.nodes.iter_mut().enumerate() {
        let Some(n) = slot else {
            continue;
        };
        let union = unions.get(id).copied().unwrap_or_default();
        add(&mut n.totals.disk, union[0])?;
        add(&mut n.totals.apparent, union[1])?;
        add(&mut n.totals.inodes, union[2])?;
        debug_assert!(n.totals.reclaim <= n.totals.charged && n.totals.reclaim <= n.totals.disk);
        debug_assert_eq!(n.totals.age.iter().sum::<u64>(), n.totals.charged);
    }
    drop(unions);
    let files = agg.candidates.select(&ids, &agg.cfg)?;
    for s in &mut agg.skipped {
        s.dir_id = *ids
            .get(s.dir_id as usize)
            .filter(|&&id| id != NONE)
            .ok_or(AggError::Tree)?;
    }
    agg.skipped.sort_by(|a, b| {
        a.dir_id
            .cmp(&b.dir_id)
            .then(a.name.as_bytes().cmp(b.name.as_bytes()))
    });
    for (_, e) in &mut agg.errors {
        e.dir_id = e.dir_id.map(|id| ids[id as usize]);
    }
    // Stable sort preserves the order of details within each directory record.
    agg.errors.sort_by_key(|(ordinal, e)| (e.dir_id, *ordinal));
    let root = &agg.nodes[0].as_ref().ok_or(AggError::Tree)?.totals;
    debug_assert_eq!(root.disk, root.charged);
    let summary = AggSummary {
        dirs: order.len() as u64,
        files: root.files,
        inodes: root.inodes,
        t_disk: root.disk,
        t_apparent: root.apparent,
        t_reclaimable: root.reclaim,
        link_sets: u64::try_from(links.len()).map_err(|_| AggError::Overflow)?,
        incomplete_link_sets: u64::try_from(links.iter().filter(|s| !s.complete).count())
            .map_err(|_| AggError::Overflow)?,
        errors: root.errors,
        races: agg.races,
        skipped: u64::try_from(agg.skipped.len()).map_err(|_| AggError::Overflow)?,
        degraded: root.errors > 0,
    };
    Ok(Prepared {
        cfg: agg.cfg,
        nodes: agg.nodes,
        order,
        ids,
        ends,
        files,
        links,
        instances,
        skipped: agg.skipped,
        errors: agg.errors.into_iter().map(|(_, e)| e).collect(),
        annotations: Vec::new(),
        summary,
        namespace_errors: None,
        ignored_annotations: 0,
        invalid_annotations: false,
    })
}

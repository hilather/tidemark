//! Owned by: W3-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Boundary path globs for hard-link analysis.
    #[arg(long, value_name = "GLOB", num_args = 1.., conflicts_with = "boundary_depth")]
    pub boundary: Vec<String>,
    /// Directory depth defining hard-link boundaries.
    #[arg(long, value_name = "N")]
    pub boundary_depth: Option<usize>,
    /// Show incomplete hard-link sets.
    #[arg(long)]
    pub incomplete: bool,
    /// Maximum number of hard-link sets.
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
    /// Resolve hard-link names live and show paths.
    #[arg(long)]
    pub show_paths: bool,
}

/// Execute hard-link topology queries over the latest containing snapshot.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use tidemark_store::links::{RegionSpec, Regions};
    let (path, snap) = super::snapshots::select(context, args.path.as_deref(), None)?;
    let top = args.top.unwrap_or(25);
    if top > 1_000_000 {
        return Err(crate::CliError::Configuration("top exceeds 1,000,000".into()).into());
    }
    let spec = if !args.boundary.is_empty() {
        Some(RegionSpec::Globs(args.boundary.clone()))
    } else {
        args.boundary_depth
            .map(|d| u32::try_from(d).map(RegionSpec::Depth))
            .transpose()
            .map_err(|_| crate::CliError::Configuration("boundary depth exceeds u32".into()))?
    };
    let mut report = tidemark_store::links::link_report(&snap, &path, spec.as_ref(), top)?;
    let regions = spec
        .as_ref()
        .map(|s| Regions::build(&snap, &path, s))
        .transpose()?;
    if args.incomplete {
        let root = snap
            .dir_by_path(&path)?
            .ok_or_else(|| anyhow::anyhow!("report root absent"))?;
        report.largest_sets = snap.link_sets_under(
            root.id,
            &tidemark_core::snapshot::LinkFilter {
                incomplete: true,
                ..Default::default()
            },
            top,
        )?;
        if let Some(regions) = &regions {
            use std::collections::BTreeMap;
            let (boundary, boundary_depth) = match &spec {
                Some(RegionSpec::Globs(g)) => (g.clone(), None),
                Some(RegionSpec::Depth(d)) => (Vec::new(), Some(*d)),
                None => (Vec::new(), None),
            };
            let sets = snap.link_sets_under(
                root.id,
                &tidemark_core::snapshot::LinkFilter {
                    incomplete: true,
                    boundary,
                    boundary_depth,
                },
                top,
            )?;
            report.crossing.clear();
            let mut count = 0usize;
            for hit in sets {
                let mut samples = BTreeMap::new();
                for i in snap.link_instances(hit.row.ino)? {
                    samples
                        .entry(regions.region_of(i.dir_id))
                        .or_insert(i.dir_id);
                }
                count += samples.len();
                if count > 1_000_000 {
                    return Err(anyhow::anyhow!("crossing sample cap reached"));
                }
                let (regions, sample_dirs) = samples.into_iter().unzip();
                report.crossing.push(tidemark_store::links::CrossingSet {
                    ino: hit.row.ino,
                    disk: hit.row.disk,
                    nlink: hit.row.nlink,
                    found: hit.row.found,
                    regions,
                    sample_dirs,
                });
            }
        }
    }

    let mut paths = Vec::new();
    let mut path_bytes = 0usize;
    if args.show_paths {
        let scan_gate = gentle_gate(&path, context)?;
        let gate = scan_gate.gate();
        let inodes: std::collections::BTreeSet<_> = report
            .largest_sets
            .iter()
            .map(|h| h.row.ino)
            .chain(report.crossing.iter().map(|s| s.ino))
            .collect();
        for ino in inodes {
            for instance in snap.link_instances(ino)? {
                if paths.len() == 1_000_000 {
                    return Err(anyhow::anyhow!("instance path cap reached"));
                }
                let directory = snap.dir(instance.dir_id)?;
                let dir = directory.path;
                if let Some(name) = instance.name {
                    push_path(
                        &mut paths,
                        &mut path_bytes,
                        instance.ino,
                        dir.join(&name),
                        "snapshot",
                    )?;
                } else {
                    match tidemark_fs::walk::read_dir_entries_checked(
                        &snap.meta().root,
                        snap.meta().fs_dev,
                        &dir,
                        Some(directory.ino),
                        gate.as_ref(),
                    ) {
                        Ok(entries) => {
                            let mut found = false;
                            for (name, ino, _) in entries {
                                if ino == instance.ino {
                                    found = true;
                                    push_path(
                                        &mut paths,
                                        &mut path_bytes,
                                        ino,
                                        dir.join(&name),
                                        "live",
                                    )?;
                                    if paths.len() > 1_000_000 {
                                        return Err(anyhow::anyhow!("instance path cap reached"));
                                    }
                                }
                            }
                            if !found {
                                push_path(&mut paths, &mut path_bytes, instance.ino, dir, "gone")?;
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::NotFound
                                    | std::io::ErrorKind::CrossesDevices
                                    | std::io::ErrorKind::NotADirectory
                            ) || e.raw_os_error()
                                == Some(rustix::io::Errno::LOOP.raw_os_error()) =>
                        {
                            push_path(&mut paths, &mut path_bytes, instance.ino, dir, "gone")?
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        paths.sort_unstable();
        paths.dedup();
        scan_gate.stop();
    }
    let mut out = std::io::stdout().lock();
    if !context.json {
        crate::render::tree::snapshot_header(
            snap.meta(),
            tidemark_core::time::unix_now(),
            &mut out,
        )?;
    }
    crate::render::links::report(
        &report,
        Some((snap.meta(), tidemark_core::time::unix_now())),
        regions.as_ref(),
        &paths,
        context.json,
        &mut out,
    )
}
pub(crate) fn gentle_gate(
    path: &tidemark_core::path::RawPath,
    context: &crate::Context,
) -> anyhow::Result<tidemark_fs::throttle::ScanGate> {
    let mounts = tidemark_fs::mount::read_mountinfo()?;
    let mount = tidemark_fs::mount::mount_for_path(&mounts, path)
        .ok_or_else(|| crate::CliError::Precondition("root mount unknown".into()))?;
    let leaves = tidemark_fs::blockdev::resolve_leaves(mount.dev).unwrap_or_default();
    let samples = leaves.clone();
    Ok(tidemark_fs::throttle::gate_from_config(
        "gentle",
        &context.config.throttle,
        &leaves,
        Box::new(move || tidemark_fs::blockdev::read_disk_stats(&samples)),
    )?)
}

fn push_path<'a>(
    paths: &mut Vec<(u64, tidemark_core::path::RawPath, &'a str)>,
    bytes: &mut usize,
    ino: u64,
    path: tidemark_core::path::RawPath,
    status: &'a str,
) -> anyhow::Result<()> {
    *bytes = bytes
        .checked_add(path.as_bytes().len())
        .ok_or_else(|| anyhow::anyhow!("instance path byte overflow"))?;
    if paths.len() >= 1_000_000 || *bytes > 64 * 1024 * 1024 {
        return Err(anyhow::anyhow!(
            "instance paths exceed one million entries or 64 MiB of path bytes"
        ));
    }
    paths.push((ino, path, status));
    Ok(())
}

//! Owned by: W1-A
//! Bounded sysfs block-device graph discovery and ordered disk-stat sampling.

use std::{
    collections::BTreeSet,
    io::{self, Read},
    path::{Path, PathBuf},
};
use tidemark_core::{
    dev::{BlockDevInfo, DevKind, DiskStat},
    scan::DevId,
};

// Hard cap includes both graph nodes and pending edges; sysfs graphs are normally tiny.
const MAX_DEVICES: usize = 4096;
/// Resolve partitions and stacked dm/md devices to sorted, unique whole-disk leaves.
pub fn resolve_leaves(dev: DevId) -> io::Result<Vec<BlockDevInfo>> {
    resolve_details(dev).map(|(leaves, _)| leaves)
}
pub(crate) fn resolve_details(dev: DevId) -> io::Result<(Vec<BlockDevInfo>, Vec<String>)> {
    resolve_graph_at(
        Path::new("/sys/dev/block"),
        libc::major(dev),
        libc::minor(dev),
    )
}

fn read_sysfs(path: impl AsRef<Path>) -> io::Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_string(&mut text)?;
    if text.len() > 65536 {
        return Err(invalid("sysfs attribute exceeds 64 KiB"));
    }
    Ok(text)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn device_number(text: &str) -> io::Result<(u32, u32)> {
    let (major, minor) = text
        .trim()
        .split_once(':')
        .ok_or_else(|| invalid("invalid sysfs device number"))?;
    Ok((
        major.parse().map_err(|_| invalid("invalid major"))?,
        minor.parse().map_err(|_| invalid("invalid minor"))?,
    ))
}
#[cfg(test)]
fn resolve_at(base: &Path, major: u32, minor: u32) -> io::Result<Vec<BlockDevInfo>> {
    resolve_graph_at(base, major, minor).map(|(leaves, _)| leaves)
}
fn resolve_graph_at(
    base: &Path,
    major: u32,
    minor: u32,
) -> io::Result<(Vec<BlockDevInfo>, Vec<String>)> {
    let mut pending = vec![base.join(format!("{major}:{minor}"))];
    let mut seen = BTreeSet::new();
    let mut leaves = Vec::new();
    let mut raid_levels = Vec::new();
    while let Some(path) = pending.pop() {
        let mut path = std::fs::canonicalize(path)?;
        // Sysfs symlinks describe device topology, not scanned filesystem paths.
        match std::fs::metadata(path.join("partition")) {
            Ok(_) => {
                path = path
                    .parent()
                    .ok_or_else(|| invalid("partition has no parent"))?
                    .to_path_buf();
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let (major, minor) = device_number(&read_sysfs(path.join("dev"))?)?;
        if !seen.insert((major, minor)) {
            continue;
        }
        if seen.len() > MAX_DEVICES {
            return Err(invalid("block-device graph exceeds 4096 nodes"));
        }
        let md_level = match read_sysfs(path.join("md/level")) {
            Ok(level) => {
                let level = level.trim().to_owned();
                raid_levels.push(format!("{major}:{minor} {level}"));
                Some(level)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let mut slaves = Vec::new();
        match std::fs::read_dir(path.join("slaves")) {
            Ok(entries) => {
                for entry in entries {
                    slaves.push(entry?.path());
                    if slaves.len() + pending.len() + seen.len() > MAX_DEVICES {
                        return Err(invalid("block-device graph exceeds 4096 edges"));
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if !slaves.is_empty() {
            slaves.sort();
            pending.extend(slaves);
            continue;
        }
        let name = path
            .file_name()
            .ok_or_else(|| invalid("device name missing"))?
            .to_string_lossy()
            .into_owned();
        let kind = if let Some(level) = md_level {
            DevKind::Md { level }
        } else if path.join("dm").exists() {
            DevKind::Dm
        } else {
            DevKind::Disk
        };
        let rotational = read_sysfs(path.join("queue/rotational"))
            .ok()
            .and_then(|s| match s.trim() {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            });
        let scheduler = read_sysfs(path.join("queue/scheduler"))
            .ok()
            .and_then(|s| parse_scheduler(&s));
        leaves.push(BlockDevInfo {
            name,
            major,
            minor,
            kind,
            rotational,
            scheduler,
        });
    }
    leaves.sort_by(|a, b| {
        a.name
            .as_bytes()
            .cmp(b.name.as_bytes())
            .then((a.major, a.minor).cmp(&(b.major, b.minor)))
    });
    if leaves.is_empty() {
        return Err(invalid("block-device graph has no leaves"));
    }
    raid_levels.sort();
    Ok((leaves, raid_levels))
}
fn parse_scheduler(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|s| {
        s.strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .map(str::to_owned)
    })
}
/// Parse the first eleven counters and validate optional discard/flush counters.
pub fn parse_disk_stat(line: &str) -> Option<DiskStat> {
    let mut values = [0; 11];
    let mut count = 0;
    if line.len() > 65536 {
        return None;
    }
    for word in line.split_whitespace() {
        let value = word.parse::<u64>().ok()?;
        if count < values.len() {
            values[count] = value;
        }
        count += 1;
    }
    if count < 11 {
        return None;
    }
    Some(DiskStat {
        read_ios: values[0],
        read_merges: values[1],
        read_sectors: values[2],
        read_ticks_ms: values[3],
        write_ios: values[4],
        write_merges: values[5],
        write_sectors: values[6],
        write_ticks_ms: values[7],
        in_flight: values[8],
        io_ticks_ms: values[9],
        time_in_queue_ms: values[10],
    })
}
/// Read exactly one sample per requested device, preserving order and duplicates.
/// Any missing or malformed sample fails the entire call.
pub fn read_disk_stats(devs: &[BlockDevInfo]) -> io::Result<Vec<DiskStat>> {
    read_stats_at(Path::new("/sys/dev/block"), devs)
}
fn read_stats_at(base: &Path, devs: &[BlockDevInfo]) -> io::Result<Vec<DiskStat>> {
    if devs.len() > MAX_DEVICES {
        return Err(invalid("too many requested disk samples"));
    }
    devs.iter()
        .map(|dev| {
            let path: PathBuf = base
                .join(format!("{}:{}", dev.major, dev.minor))
                .join("stat");
            parse_disk_stat(&read_sysfs(&path)?)
                .ok_or_else(|| invalid(&format!("invalid disk stat: {}", path.display())))
        })
        .collect()
}
#[cfg(test)]
#[path = "../tests/w1a/blockdev.rs"]
mod tests;

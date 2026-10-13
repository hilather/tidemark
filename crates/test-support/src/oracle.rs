//! Owned by: W0-C
//! GNU du/find and reporting-command reference numbers, without path text parsing.
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    io::{self, Read},
    path::Path,
    process::{Command, Stdio},
};
fn parse_error(s: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s.into())
}
/// Check GNU tool availability. Call before oracle tests; prints a skip reason.
pub fn gnu_available(tool: &str) -> bool {
    match output(tool, &[OsStr::new("--version")]) {
        Ok(o) if String::from_utf8_lossy(&o).contains("GNU") => true,
        _ => {
            eprintln!("skipping oracle: GNU {tool} is unavailable");
            false
        }
    }
}
/// Maximum captured tool output, bounding both buffers and parsed inode sets.
pub const MAX_ORACLE_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;
fn output(tool: &str, args: &[&OsStr]) -> io::Result<Vec<u8>> {
    let mut child = Command::new(tool)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("missing oracle stdout"));
    };
    let mut bytes = Vec::new();
    let read = stdout
        .take(MAX_ORACLE_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > MAX_ORACLE_OUTPUT_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        read?;
        return Err(io::Error::other("oracle output exceeds 32 MiB cap"));
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "{tool} failed: {status}; stderr streamed above"
        )));
    }
    Ok(bytes)
}
fn du(path: &Path, apparent: bool) -> io::Result<u64> {
    let mut args = vec![OsStr::new("-sxB1")];
    if apparent {
        args.push(OsStr::new("--apparent-size"));
    }
    args.extend([OsStr::new("--"), path.as_os_str()]);
    let o = output("du", &args)?;
    let field = o
        .split(|b| b.is_ascii_whitespace())
        .next()
        .ok_or_else(|| parse_error("empty du output"))?;
    std::str::from_utf8(field)
        .map_err(|e| parse_error(e.to_string()))?
        .parse()
        .map_err(|e: std::num::ParseIntError| parse_error(e.to_string()))
}
/// Fallible allocated-byte oracle. Use `gnu_available` to skip unavailable tools.
pub fn try_du_disk(path: &Path) -> io::Result<u64> {
    du(path, false)
}
/// GNU `du -sxB1` allocated bytes. Call only after `gnu_available("du")`.
pub fn du_disk(path: impl AsRef<Path>) -> u64 {
    du(path.as_ref(), false).unwrap_or_else(|e| panic!("du oracle: {e}"))
}
/// GNU `du -sxB1 --apparent-size` apparent bytes.
pub fn du_apparent(path: impl AsRef<Path>) -> u64 {
    du(path.as_ref(), true).unwrap_or_else(|e| panic!("du apparent oracle: {e}"))
}
/// GNU apparent-size output normalized to directory-inclusive link semantics (03 §3).
/// Only metadata is inspected; symlinks and other filesystems are never traversed.
/// At most one million visited inode identities and pending paths are retained.
pub fn du_apparent_normalized(path: &Path) -> io::Result<u64> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let mut total = du(path, true)?;
    if du_apparent_counts_directories()? {
        return Ok(total);
    }
    let device = std::fs::symlink_metadata(path)?.dev();
    let mut stack = vec![path.to_owned()];
    let mut seen = BTreeSet::new();
    while let Some(path) = stack.pop() {
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.dev() != device || !seen.insert((metadata.dev(), metadata.ino())) {
            continue;
        }
        if seen.len() > 1_000_000 {
            return Err(io::Error::other(
                "apparent oracle exceeds one million inodes",
            ));
        }
        if metadata.is_dir() {
            total = total
                .checked_add(metadata.len())
                .ok_or_else(|| parse_error("apparent oracle overflow"))?;
            for entry in std::fs::read_dir(&path)? {
                if stack.len() >= 1_000_000 {
                    return Err(io::Error::other(
                        "apparent oracle exceeds one million pending paths",
                    ));
                }
                stack.push(entry?.path());
            }
        } else if metadata.file_type().is_fifo()
            || metadata.file_type().is_socket()
            || metadata.file_type().is_char_device()
            || metadata.file_type().is_block_device()
        {
            total = total
                .checked_add(metadata.len())
                .ok_or_else(|| parse_error("apparent oracle overflow"))?;
        }
    }
    Ok(total)
}
/// Entry and inode counts; symlinks are not followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindCounts {
    /// Non-directory entry instances.
    pub files: u64,
    /// Directories including the starting path.
    pub dirs: u64,
    /// Distinct non-directory inodes.
    pub inodes: u64,
}
/// GNU find counts, robust to non-UTF-8 and newline names. Fixtures contain no mounts.
/// Call after `gnu_available("find")`; scan exclusions must be accounted for by caller.
pub fn find_counts(path: impl AsRef<Path>) -> FindCounts {
    try_find_counts(path.as_ref()).unwrap_or_else(|e| panic!("find oracle: {e}"))
}
/// Fallible GNU find oracle.
pub fn try_find_counts(path: &Path) -> io::Result<FindCounts> {
    // Absolute paths cannot be mistaken for find expressions.
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let o = output(
        "find",
        &[
            path.as_os_str(),
            OsStr::new("-xdev"),
            OsStr::new("-printf"),
            OsStr::new("%y %i\n"),
        ],
    )?;
    let mut files = 0;
    let mut dirs = 0;
    let mut inodes = BTreeSet::new();
    for line in o.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        if line.first() == Some(&b'd') {
            dirs += 1;
        } else {
            files += 1;
            inodes.insert(
                line.get(2..)
                    .ok_or_else(|| parse_error("bad find output"))?
                    .to_vec(),
            );
        }
    }
    Ok(FindCounts {
        files,
        dirs,
        inodes: inodes.len() as u64,
    })
}
/// Filesystem byte totals from GNU df.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DfTotals {
    /// Total filesystem bytes.
    pub size: u64,
    /// Used bytes.
    pub used: u64,
    /// Available bytes.
    pub avail: u64,
}
/// Parse `df -B1 --output=size,used,avail` on a fixture mount.
pub fn df_totals(path: &Path) -> io::Result<DfTotals> {
    let o = output(
        "df",
        &[
            OsStr::new("-B1"),
            OsStr::new("--output=size,used,avail"),
            OsStr::new("--"),
            path.as_os_str(),
        ],
    )?;
    let s = String::from_utf8(o).map_err(|e| parse_error(e.to_string()))?;
    let fields: Vec<_> = s
        .split_whitespace()
        .skip(3)
        .map(str::parse::<u64>)
        .collect::<Result<_, _>>()
        .map_err(|e| parse_error(e.to_string()))?;
    match fields.as_slice() {
        [size, used, avail] => Ok(DfTotals {
            size: *size,
            used: *used,
            avail: *avail,
        }),
        _ => Err(parse_error("unexpected df output")),
    }
}
/// Plain numeric reporting output for LVM comparisons (no mutating commands).
pub fn lvs_bytes(vg: &str) -> io::Result<Vec<[f64; 3]>> {
    let o = output(
        "lvs",
        &[
            OsStr::new("--units"),
            OsStr::new("b"),
            OsStr::new("--nosuffix"),
            OsStr::new("--noheadings"),
            OsStr::new("--separator"),
            OsStr::new("|"),
            OsStr::new("-o"),
            OsStr::new("lv_size,data_percent,metadata_percent"),
            OsStr::new("--"),
            OsStr::new(vg),
        ],
    )?;
    let s = String::from_utf8(o).map_err(|e| parse_error(e.to_string()))?;
    s.lines()
        .map(|line| {
            let fields: Vec<_> = line
                .split('|')
                .map(|s| {
                    if s.trim().is_empty() {
                        Ok(0.0)
                    } else {
                        s.trim().parse::<f64>()
                    }
                })
                .collect::<Result<_, _>>()
                .map_err(|e| parse_error(e.to_string()))?;
            match fields.as_slice() {
                [a, b, c] => Ok([*a, *b, *c]),
                _ => Err(parse_error("bad lvs output")),
            }
        })
        .collect()
}
/// VG free-byte oracle, using only `vgs` reporting.
pub fn vgs_free(vg: &str) -> io::Result<u64> {
    let o = output(
        "vgs",
        &[
            OsStr::new("--units"),
            OsStr::new("b"),
            OsStr::new("--nosuffix"),
            OsStr::new("--noheadings"),
            OsStr::new("-o"),
            OsStr::new("vg_free"),
            OsStr::new("--"),
            OsStr::new(vg),
        ],
    )?;
    String::from_utf8(o)
        .map_err(|e| parse_error(e.to_string()))?
        .trim()
        .parse()
        .map_err(|e: std::num::ParseIntError| parse_error(e.to_string()))
}

/// Probe whether this GNU du counts directory st_size in apparent usage.
/// Some versions omit it; the normative Tidemark reference always includes it.
/// The probe uses an owned empty temporary directory and never changes metrics.
pub fn du_apparent_counts_directories() -> io::Result<bool> {
    let dir = crate::private_tempdir()?;
    let apparent = std::fs::symlink_metadata(dir.path())?.len();
    let observed = du(dir.path(), true)?;
    if observed == apparent {
        Ok(true)
    } else if observed == 0 {
        Ok(false)
    } else {
        Err(parse_error("unexpected apparent usage for empty directory"))
    }
}

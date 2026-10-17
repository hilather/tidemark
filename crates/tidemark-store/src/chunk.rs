//! Owned by: WSNAP-codec
//! Bounded, pure snapshot-v2 column codecs (15 §4).

use crate::StoreError;

/// Default chunk compression level.
pub const DEFAULT_LEVEL: i32 = 3;
/// Maximum rows accepted per chunk; writers normally use 4096 directories or 8192 files.
pub const MAX_ROWS: usize = 8192;
/// Maximum raw name length, including a root path.
pub const MAX_NAME: usize = 4096;
// 8192 maximum-size names plus fixed-width worst-case columns, rounded up.
const MAX_BYTES: usize = 40 * 1024 * 1024;
type Result<T> = std::result::Result<T, StoreError>;
fn invalid() -> StoreError {
    StoreError::Invalid("invalid snapshot chunk".into())
}

/// Directory data stored in a chunk; paths and path hashes are reconstructed by readers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChunkDir {
    /// Row identifier; directory ids are preorder indices (root = 0).
    pub id: u32,
    /// Parent directory id; None for the root.
    pub parent_id: Option<u32>,
    /// Last preorder directory id in this subtree, inclusive.
    pub subtree_end: u32,
    /// Directory depth relative to the scan root.
    pub depth: u32,
    /// Raw name bytes; the root directory row stores the full root path.
    pub name: Vec<u8>,
    /// Filesystem inode number.
    pub ino: u64,
    /// Inode modification time, Unix seconds.
    pub mtime: i64,
    /// Directory inode allocated bytes (st_blocks × 512).
    pub dir_disk: u64,
    /// Directory inode apparent bytes (st_size).
    pub dir_apparent: u64,
    /// XFS project id; None in walk mode.
    pub projid: Option<u32>,
    /// All direct entries, including subdirectories and skipped entries.
    pub entries: u32,
    /// Errors recorded directly in this directory.
    pub errors: u32,
    /// Direct non-directory entries, counting link instances.
    pub own_files: u64,
    /// Direct symlink entries (a subset of own_files).
    pub own_symlinks: u64,
    /// Directory inode bytes plus single-found inodes and link sets charged here.
    pub own_charged: u64,
    /// Apparent bytes of the same inodes as own_charged.
    pub own_apparent: u64,
    /// Subtree directory count, including this directory.
    pub t_dirs: u64,
    /// Subtree non-directory entry count, counting link instances.
    pub t_files: u64,
    /// Distinct non-directory inodes visible in the subtree.
    pub t_inodes: u64,
    /// Allocated bytes of distinct visible inodes plus directory inode bytes.
    pub t_disk: u64,
    /// Apparent bytes using the same union semantics as t_disk.
    pub t_apparent: u64,
    /// Subtree allocated bytes charged at each inode’s directory or link-set LCA.
    pub t_charged: u64,
    /// Charged bytes whose complete link sets are inside the subtree.
    pub t_reclaimable: u64,
    /// Total subtree error count.
    pub t_errors: u64,
    /// Charged bytes with mtime age less than 1 day.
    pub age0: u64,
    /// Charged bytes with mtime age 1–7 days.
    pub age1: u64,
    /// Charged bytes with mtime age 7–30 days.
    pub age2: u64,
    /// Charged bytes with mtime age 30–90 days.
    pub age3: u64,
    /// Charged bytes with mtime age 90–365 days.
    pub age4: u64,
    /// Charged bytes with mtime age at least 365 days.
    pub age5: u64,
    /// Newest mtime of every visible inode in the subtree, Unix seconds.
    pub newest_mtime: i64,
    /// Largest direct-entry count of any subtree directory.
    pub max_entries: u32,
    /// First notable-file row for this directory.
    pub file_lo: u32,
    /// Number of notable-file rows.
    pub file_count: u32,
}
/// Notable-file data; the row id is supplied by the chunk table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChunkFile {
    /// Stored dir id.
    pub dir_id: u32,
    /// Stored name.
    pub name: Option<Vec<u8>>,
    /// Stored ino.
    pub ino: u64,
    /// Stored disk.
    pub disk: u64,
    /// Stored apparent.
    pub apparent: u64,
    /// Stored mtime.
    pub mtime: i64,
    /// Stored nlink.
    pub nlink: u32,
}
/// Link-instance data in inode, directory, name order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChunkLinkInstance {
    /// Stored ino.
    pub ino: u64,
    /// Stored dir id.
    pub dir_id: u32,
    /// Stored name.
    pub name: Option<Vec<u8>>,
}

/// Lookup hash width recorded in snapshot metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashWidth {
    /// Low 32 bits, stored as a nonnegative SQLite integer.
    Bits32,
    /// All 64 bits, reinterpreted as a signed SQLite integer.
    Bits64,
}
impl HashWidth {
    /// Validate a metadata width.
    pub fn new(bits: u32) -> Result<Self> {
        match bits {
            32 => Ok(Self::Bits32),
            64 => Ok(Self::Bits64),
            _ => Err(invalid()),
        }
    }
    /// Convert an already computed xxh3 hash to its SQLite lookup key.
    pub fn key(self, hash: u64) -> i64 {
        match self {
            Self::Bits32 => i64::from(hash as u32),
            Self::Bits64 => i64::from_ne_bytes(hash.to_ne_bytes()),
        }
    }
    /// Width to record in metadata.
    pub fn bits(self) -> u32 {
        match self {
            Self::Bits32 => 32,
            Self::Bits64 => 64,
        }
    }
}

fn put(mut n: u128, out: &mut Vec<u8>) {
    loop {
        let byte = (n & 127) as u8;
        n >>= 7;
        out.push(byte | if n == 0 { 0 } else { 128 });
        if n == 0 {
            break;
        }
    }
}
#[derive(Clone, Copy)]
struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn var(&mut self) -> Result<u128> {
        let mut n = 0;
        // Deltas between two i64 timestamps require up to 65 zig-zag bits.
        for i in 0..10 {
            let (&b, tail) = self.0.split_first().ok_or_else(invalid)?;
            self.0 = tail;
            if i == 9 && b > 3 {
                return Err(invalid());
            }
            n |= u128::from(b & 127) << (i * 7);
            if b < 128 {
                if i > 0 && b == 0 {
                    return Err(invalid());
                }
                return Ok(n);
            }
        }
        Err(invalid())
    }
    fn name(&mut self, optional: bool) -> Result<Option<&'a [u8]>> {
        let n = self.var()?;
        if optional && n == 0 {
            return Ok(None);
        }
        let n = usize::try_from(n - u128::from(optional)).map_err(|_| invalid())?;
        if n > MAX_NAME || n > self.0.len() {
            return Err(invalid());
        }
        let (v, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(Some(v))
    }
}
fn unsigned(n: u128) -> Result<u64> {
    if n > i64::MAX as u128 {
        Err(invalid())
    } else {
        Ok(n as u64)
    }
}
fn narrow(n: u128) -> Result<u32> {
    u32::try_from(n).map_err(|_| invalid())
}
fn signed(n: u128) -> i128 {
    if n & 1 == 0 {
        (n >> 1) as i128
    } else {
        -((n >> 1) as i128) - 1
    }
}
fn zig(n: i128) -> u128 {
    if n >= 0 {
        (n as u128) << 1
    } else {
        ((-n) as u128) * 2 - 1
    }
}
fn time(n: i128) -> Result<i64> {
    i64::try_from(n).map_err(|_| invalid())
}
fn name_put(name: Option<&[u8]>, optional: bool, out: &mut Vec<u8>) -> Result<()> {
    match name {
        None if optional => put(0, out),
        Some(n) if n.len() <= MAX_NAME => {
            put(n.len() as u128 + u128::from(optional), out);
            out.extend_from_slice(n);
        }
        _ => return Err(invalid()),
    }
    Ok(())
}
fn frame(cols: Vec<Vec<u8>>, level: i32) -> Result<Vec<u8>> {
    if !(1..=19).contains(&level) {
        return Err(invalid());
    }
    let size: usize = cols.iter().map(Vec::len).sum();
    if size > MAX_BYTES {
        return Err(invalid());
    }
    let mut out = vec![1];
    put(cols.len() as u128, &mut out);
    let mut raw = Vec::with_capacity(size);
    for c in cols {
        put(c.len() as u128, &mut out);
        raw.extend_from_slice(&c);
    }
    out.extend_from_slice(&zstd::bulk::compress(&raw, level).map_err(|_| invalid())?);
    Ok(out)
}
struct Columns {
    raw: Vec<u8>,
    lengths: Vec<usize>,
}
impl Columns {
    fn open(data: &[u8], count: usize) -> Result<Self> {
        if data.len() > MAX_BYTES + 1024 * 1024 {
            return Err(invalid());
        }
        let (&version, rest) = data.split_first().ok_or_else(invalid)?;
        if version != 1 {
            return Err(invalid());
        }
        let mut header = Cursor(rest);
        if header.var()? != count as u128 {
            return Err(invalid());
        }
        let mut lengths = Vec::with_capacity(count);
        let mut total = 0usize;
        for _ in 0..count {
            let n = usize::try_from(header.var()?).map_err(|_| invalid())?;
            total = total
                .checked_add(n)
                .filter(|n| *n <= MAX_BYTES)
                .ok_or_else(invalid)?;
            lengths.push(n);
        }
        // Reject concatenated frames, skippable frames, and trailing bytes.
        if !header.0.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
            || zstd::zstd_safe::find_frame_compressed_size(header.0).map_err(|_| invalid())?
                != header.0.len()
        {
            return Err(invalid());
        }
        let mut decoder = zstd::bulk::Decompressor::new().map_err(|_| invalid())?;
        decoder
            .set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(26))
            .map_err(|_| invalid())?;
        let raw = decoder.decompress(header.0, total).map_err(|_| invalid())?;
        if raw.len() != total {
            return Err(invalid());
        }
        Ok(Self { raw, lengths })
    }
    fn cursors(&self) -> Vec<Cursor<'_>> {
        let mut offset = 0;
        self.lengths
            .iter()
            .map(|&n| {
                let c = Cursor(&self.raw[offset..offset + n]);
                offset += n;
                c
            })
            .collect()
    }
}
fn finish(c: &[Cursor<'_>]) -> Result<()> {
    if c.iter().any(|c| !c.0.is_empty()) {
        Err(invalid())
    } else {
        Ok(())
    }
}

/// Encode consecutive preorder directories at the default compression level.
pub fn encode_dirs(rows: &[ChunkDir]) -> Result<Vec<u8>> {
    encode_dirs_with_level(rows, DEFAULT_LEVEL)
}
/// Encode consecutive preorder directories with a writer-selected level (1..=19).
pub fn encode_dirs_with_level(rows: &[ChunkDir], level: i32) -> Result<Vec<u8>> {
    if rows.len() > MAX_ROWS {
        return Err(invalid());
    }
    let mut cols = vec![Vec::new(); 33];
    let (mut ino, mut mtime, mut file_lo) = (0i128, 0i128, 0u32);
    for (index, r) in rows.iter().enumerate() {
        if r.id
            != rows[0]
                .id
                .checked_add(u32::try_from(index).map_err(|_| invalid())?)
                .ok_or_else(invalid)?
        {
            return Err(invalid());
        }
        let parent = match (r.id, r.parent_id) {
            (0, None) => 0,
            (id, Some(p)) if p < id => id - p,
            _ => return Err(invalid()),
        };
        put(u128::from(parent), &mut cols[0]);
        put(
            u128::from(r.subtree_end.checked_sub(r.id).ok_or_else(invalid)?),
            &mut cols[1],
        );
        put(u128::from(r.depth), &mut cols[2]);
        name_put(Some(&r.name), false, &mut cols[3])?;
        unsigned(u128::from(r.ino))?;
        put(zig(i128::from(r.ino) - ino), &mut cols[4]);
        put(zig(i128::from(r.mtime) - mtime), &mut cols[5]);
        put(
            zig(i128::from(r.newest_mtime) - i128::from(r.mtime)),
            &mut cols[6],
        );
        put(r.projid.map_or(0, |p| u128::from(p) + 1), &mut cols[7]);
        unsigned(u128::from(r.dir_disk))?;
        put(u128::from(r.dir_disk), &mut cols[8]);
        unsigned(u128::from(r.dir_apparent))?;
        put(u128::from(r.dir_apparent), &mut cols[9]);
        put(u128::from(r.entries), &mut cols[10]);
        put(u128::from(r.errors), &mut cols[11]);
        unsigned(u128::from(r.own_files))?;
        put(u128::from(r.own_files), &mut cols[12]);
        unsigned(u128::from(r.own_symlinks))?;
        put(u128::from(r.own_symlinks), &mut cols[13]);
        unsigned(u128::from(r.own_charged))?;
        put(u128::from(r.own_charged), &mut cols[14]);
        unsigned(u128::from(r.own_apparent))?;
        put(u128::from(r.own_apparent), &mut cols[15]);
        unsigned(u128::from(r.t_dirs))?;
        put(u128::from(r.t_dirs), &mut cols[16]);
        unsigned(u128::from(r.t_files))?;
        put(u128::from(r.t_files), &mut cols[17]);
        unsigned(u128::from(r.t_inodes))?;
        put(u128::from(r.t_inodes), &mut cols[18]);
        unsigned(u128::from(r.t_disk))?;
        put(u128::from(r.t_disk), &mut cols[19]);
        unsigned(u128::from(r.t_apparent))?;
        put(u128::from(r.t_apparent), &mut cols[20]);
        unsigned(u128::from(r.t_charged))?;
        put(u128::from(r.t_charged), &mut cols[21]);
        unsigned(u128::from(r.t_reclaimable))?;
        put(u128::from(r.t_reclaimable), &mut cols[22]);
        unsigned(u128::from(r.t_errors))?;
        put(u128::from(r.t_errors), &mut cols[23]);
        unsigned(u128::from(r.age0))?;
        put(u128::from(r.age0), &mut cols[24]);
        unsigned(u128::from(r.age1))?;
        put(u128::from(r.age1), &mut cols[25]);
        unsigned(u128::from(r.age2))?;
        put(u128::from(r.age2), &mut cols[26]);
        unsigned(u128::from(r.age3))?;
        put(u128::from(r.age3), &mut cols[27]);
        unsigned(u128::from(r.age4))?;
        put(u128::from(r.age4), &mut cols[28]);
        unsigned(u128::from(r.age5))?;
        put(u128::from(r.age5), &mut cols[29]);
        put(u128::from(r.max_entries), &mut cols[30]);
        put(
            u128::from(r.file_lo.checked_sub(file_lo).ok_or_else(invalid)?),
            &mut cols[31],
        );
        put(u128::from(r.file_count), &mut cols[32]);
        ino = i128::from(r.ino);
        mtime = i128::from(r.mtime);
        file_lo = r.file_lo;
    }
    frame(cols, level)
}

/// Hot directory sort columns, in chunk row order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirProjection {
    /// Values of t_disk.
    pub t_disk: Vec<u64>,
    /// Values of t_reclaimable.
    pub t_reclaimable: Vec<u64>,
    /// Values of t_files.
    pub t_files: Vec<u64>,
    /// Values of newest_mtime.
    pub newest_mtime: Vec<i64>,
    /// Values of entries.
    pub entries: Vec<u32>,
}
/// Decode directory rows; first_id is supplied by the chunk table.
pub fn decode_dirs(data: &[u8], first_id: u32) -> Result<Vec<ChunkDir>> {
    let mut out = Vec::new();
    visit_dirs(data, first_id, true, |r, _| {
        if let Some(row) = r {
            out.push(row);
        }
    })?;
    Ok(out)
}
/// Decode hot columns without allocating names or retaining full rows.
/// Every column is still validated, including columns not projected.
pub fn decode_dir_projection(data: &[u8], first_id: u32) -> Result<DirProjection> {
    let mut out = DirProjection::default();
    visit_dirs(data, first_id, false, |_, r| {
        out.t_disk.push(r.t_disk);
        out.t_reclaimable.push(r.t_reclaimable);
        out.t_files.push(r.t_files);
        out.newest_mtime.push(r.newest_mtime);
        out.entries.push(r.entries);
    })?;
    Ok(out)
}
struct HotRow {
    t_disk: u64,
    t_reclaimable: u64,
    t_files: u64,
    newest_mtime: i64,
    entries: u32,
}
fn visit_dirs(
    data: &[u8],
    first_id: u32,
    names: bool,
    mut visit: impl FnMut(Option<ChunkDir>, HotRow),
) -> Result<()> {
    let columns = Columns::open(data, 33)?;
    let mut c = columns.cursors();
    let (mut ino, mut mtime, mut file_lo) = (0i128, 0i128, 0u32);
    let mut count = 0usize;
    while !c[0].0.is_empty() {
        if count == MAX_ROWS {
            return Err(invalid());
        }
        let id = first_id.checked_add(count as u32).ok_or_else(invalid)?;
        let delta = narrow(c[0].var()?)?;
        let parent_id = if id == 0 {
            if delta != 0 {
                return Err(invalid());
            }
            None
        } else {
            if delta == 0 {
                return Err(invalid());
            }
            Some(id.checked_sub(delta).ok_or_else(invalid)?)
        };
        let subtree_end = id.checked_add(narrow(c[1].var()?)?).ok_or_else(invalid)?;
        let depth = narrow(c[2].var()?)?;
        let name = c[3].name(false)?.ok_or_else(invalid)?;
        ino += signed(c[4].var()?);
        let ino_value = unsigned(u128::try_from(ino).map_err(|_| invalid())?)?;
        mtime += signed(c[5].var()?);
        let mtime_value = time(mtime)?;
        let newest_mtime = time(mtime + signed(c[6].var()?))?;
        let p = c[7].var()?;
        let projid = if p == 0 { None } else { Some(narrow(p - 1)?) };
        let dir_disk = unsigned(c[8].var()?)?;
        let dir_apparent = unsigned(c[9].var()?)?;
        let entries = narrow(c[10].var()?)?;
        let errors = narrow(c[11].var()?)?;
        let own_files = unsigned(c[12].var()?)?;
        let own_symlinks = unsigned(c[13].var()?)?;
        let own_charged = unsigned(c[14].var()?)?;
        let own_apparent = unsigned(c[15].var()?)?;
        let t_dirs = unsigned(c[16].var()?)?;
        let t_files = unsigned(c[17].var()?)?;
        let t_inodes = unsigned(c[18].var()?)?;
        let t_disk = unsigned(c[19].var()?)?;
        let t_apparent = unsigned(c[20].var()?)?;
        let t_charged = unsigned(c[21].var()?)?;
        let t_reclaimable = unsigned(c[22].var()?)?;
        let t_errors = unsigned(c[23].var()?)?;
        let age0 = unsigned(c[24].var()?)?;
        let age1 = unsigned(c[25].var()?)?;
        let age2 = unsigned(c[26].var()?)?;
        let age3 = unsigned(c[27].var()?)?;
        let age4 = unsigned(c[28].var()?)?;
        let age5 = unsigned(c[29].var()?)?;
        let max_entries = narrow(c[30].var()?)?;
        file_lo = file_lo
            .checked_add(narrow(c[31].var()?)?)
            .ok_or_else(invalid)?;
        let file_count = narrow(c[32].var()?)?;
        let hot = HotRow {
            t_disk,
            t_reclaimable,
            t_files,
            newest_mtime,
            entries,
        };
        let row = if names {
            Some(ChunkDir {
                id,
                parent_id,
                subtree_end,
                depth,
                name: name.to_vec(),
                ino: ino_value,
                mtime: mtime_value,
                newest_mtime,
                projid,
                dir_disk,
                dir_apparent,
                entries,
                errors,
                own_files,
                own_symlinks,
                own_charged,
                own_apparent,
                t_dirs,
                t_files,
                t_inodes,
                t_disk,
                t_apparent,
                t_charged,
                t_reclaimable,
                t_errors,
                age0,
                age1,
                age2,
                age3,
                age4,
                age5,
                max_entries,
                file_lo,
                file_count,
            })
        } else {
            None
        };
        visit(row, hot);
        count += 1;
    }
    finish(&c)
}

/// Encode notable-file rows in (dir_id, name) order.
pub fn encode_files(rows: &[ChunkFile]) -> Result<Vec<u8>> {
    encode_files_with_level(rows, DEFAULT_LEVEL)
}
/// Encode notable-file rows using a writer-selected compression level.
pub fn encode_files_with_level(rows: &[ChunkFile], level: i32) -> Result<Vec<u8>> {
    if rows.len() > MAX_ROWS
        || rows
            .windows(2)
            .any(|w| (w[0].dir_id, &w[0].name) > (w[1].dir_id, &w[1].name))
    {
        return Err(invalid());
    }
    let mut c = vec![Vec::new(); 7];
    let (mut dir, mut ino, mut mtime) = (0u32, 0i128, 0i128);
    for r in rows {
        put(
            u128::from(r.dir_id.checked_sub(dir).ok_or_else(invalid)?),
            &mut c[0],
        );
        name_put(r.name.as_deref(), true, &mut c[1])?;
        unsigned(u128::from(r.ino))?;
        put(zig(i128::from(r.ino) - ino), &mut c[2]);
        put(u128::from(unsigned(u128::from(r.disk))?), &mut c[3]);
        put(u128::from(unsigned(u128::from(r.apparent))?), &mut c[4]);
        put(zig(i128::from(r.mtime) - mtime), &mut c[5]);
        put(u128::from(r.nlink), &mut c[6]);
        dir = r.dir_id;
        ino = i128::from(r.ino);
        mtime = i128::from(r.mtime);
    }
    frame(c, level)
}
/// Decode notable-file rows; ids are their clustered row numbers in the enclosing table.
pub fn decode_files(data: &[u8]) -> Result<Vec<ChunkFile>> {
    let columns = Columns::open(data, 7)?;
    let mut c = columns.cursors();
    let (mut dir, mut ino, mut mtime) = (0u32, 0i128, 0i128);
    let mut out: Vec<ChunkFile> = Vec::new();
    while !c[0].0.is_empty() {
        if out.len() == MAX_ROWS {
            return Err(invalid());
        }
        dir = dir.checked_add(narrow(c[0].var()?)?).ok_or_else(invalid)?;
        let name = c[1].name(true)?.map(<[u8]>::to_vec);
        ino += signed(c[2].var()?);
        let ino_value = unsigned(u128::try_from(ino).map_err(|_| invalid())?)?;
        let disk = unsigned(c[3].var()?)?;
        let apparent = unsigned(c[4].var()?)?;
        mtime += signed(c[5].var()?);
        let row = ChunkFile {
            dir_id: dir,
            name,
            ino: ino_value,
            disk,
            apparent,
            mtime: time(mtime)?,
            nlink: narrow(c[6].var()?)?,
        };
        if out
            .last()
            .is_some_and(|p| (p.dir_id, &p.name) > (row.dir_id, &row.name))
        {
            return Err(invalid());
        }
        out.push(row);
    }
    finish(&c)?;
    Ok(out)
}

/// Encode link instances in (ino, dir_id, name) order.
pub fn encode_link_instances(rows: &[ChunkLinkInstance]) -> Result<Vec<u8>> {
    encode_link_instances_with_level(rows, DEFAULT_LEVEL)
}
/// Encode link instances using a writer-selected compression level.
pub fn encode_link_instances_with_level(rows: &[ChunkLinkInstance], level: i32) -> Result<Vec<u8>> {
    if rows.len() > MAX_ROWS
        || rows
            .windows(2)
            .any(|w| (w[0].ino, w[0].dir_id, &w[0].name) > (w[1].ino, w[1].dir_id, &w[1].name))
    {
        return Err(invalid());
    }
    let mut c = vec![Vec::new(); 3];
    let (mut ino, mut dir) = (0u64, 0u32);
    for r in rows {
        unsigned(u128::from(r.ino))?;
        put(
            u128::from(r.ino.checked_sub(ino).ok_or_else(invalid)?),
            &mut c[0],
        );
        put(zig(i128::from(r.dir_id) - i128::from(dir)), &mut c[1]);
        name_put(r.name.as_deref(), true, &mut c[2])?;
        ino = r.ino;
        dir = r.dir_id;
    }
    frame(c, level)
}
/// Decode link instances, validating ordering and all integer widths.
pub fn decode_link_instances(data: &[u8]) -> Result<Vec<ChunkLinkInstance>> {
    let columns = Columns::open(data, 3)?;
    let mut c = columns.cursors();
    let (mut ino, mut dir) = (0u64, 0i128);
    let mut out: Vec<ChunkLinkInstance> = Vec::new();
    while !c[0].0.is_empty() {
        if out.len() == MAX_ROWS {
            return Err(invalid());
        }
        ino = ino
            .checked_add(unsigned(c[0].var()?)?)
            .ok_or_else(invalid)?;
        unsigned(u128::from(ino))?;
        dir += signed(c[1].var()?);
        let row = ChunkLinkInstance {
            ino,
            dir_id: u32::try_from(dir).map_err(|_| invalid())?,
            name: c[2].name(true)?.map(<[u8]>::to_vec),
        };
        if out
            .last()
            .is_some_and(|p| (p.ino, p.dir_id, &p.name) > (row.ino, row.dir_id, &row.name))
        {
            return Err(invalid());
        }
        out.push(row);
    }
    finish(&c)?;
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sql_value() -> impl Strategy<Value = u64> {
        prop_oneof![Just(0), Just(i64::MAX as u64), 0..=i64::MAX as u64]
    }
    fn raw_name() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            Just(Vec::new()),
            proptest::collection::vec(any::<u8>(), 0..64),
            proptest::collection::vec(any::<u8>(), 4096)
        ]
    }
    fn file() -> impl Strategy<Value = ChunkFile> {
        (
            any::<u32>(),
            proptest::option::of(raw_name()),
            sql_value(),
            sql_value(),
            sql_value(),
            any::<i64>(),
            any::<u32>(),
        )
            .prop_map(
                |(dir_id, name, ino, disk, apparent, mtime, nlink)| ChunkFile {
                    dir_id,
                    name,
                    ino,
                    disk,
                    apparent,
                    mtime,
                    nlink,
                },
            )
    }
    fn link() -> impl Strategy<Value = ChunkLinkInstance> {
        (sql_value(), any::<u32>(), proptest::option::of(raw_name()))
            .prop_map(|(ino, dir_id, name)| ChunkLinkInstance { ino, dir_id, name })
    }
    fn dir() -> impl Strategy<Value = ChunkDir> {
        (
            raw_name(),
            sql_value(),
            any::<i64>(),
            any::<i64>(),
            proptest::option::of(any::<u32>()),
            proptest::collection::vec(sql_value(), 22),
            any::<u32>(),
        )
            .prop_map(
                |(name, ino, mtime, newest_mtime, projid, v, count)| ChunkDir {
                    name,
                    ino,
                    mtime,
                    newest_mtime,
                    projid,
                    entries: count,
                    errors: count,
                    max_entries: count,
                    dir_disk: v[0],
                    dir_apparent: v[1],
                    own_files: v[2],
                    own_symlinks: v[3],
                    own_charged: v[4],
                    own_apparent: v[5],
                    t_dirs: v[6],
                    t_files: v[7],
                    t_inodes: v[8],
                    t_disk: v[9],
                    t_apparent: v[10],
                    t_charged: v[11],
                    t_reclaimable: v[12],
                    t_errors: v[13],
                    age0: v[14],
                    age1: v[15],
                    age2: v[16],
                    age3: v[17],
                    age4: v[18],
                    age5: v[19],
                    file_count: count,
                    ..ChunkDir::default()
                },
            )
    }
    fn sequence(rows: &mut [ChunkDir], first: u32) {
        for (i, r) in rows.iter_mut().enumerate() {
            r.id = first + i as u32;
            r.parent_id = if r.id == 0 { None } else { Some(r.id - 1) };
            r.subtree_end = r.id;
            r.depth = r.id;
            r.file_lo = i as u32;
        }
    }
    fn projection(data: &[u8], first: u32, rows: &[ChunkDir]) {
        let p = decode_dir_projection(data, first).unwrap();
        assert_eq!(p.t_disk, rows.iter().map(|r| r.t_disk).collect::<Vec<_>>());
        assert_eq!(
            p.t_reclaimable,
            rows.iter().map(|r| r.t_reclaimable).collect::<Vec<_>>()
        );
        assert_eq!(
            p.t_files,
            rows.iter().map(|r| r.t_files).collect::<Vec<_>>()
        );
        assert_eq!(
            p.newest_mtime,
            rows.iter().map(|r| r.newest_mtime).collect::<Vec<_>>()
        );
        assert_eq!(
            p.entries,
            rows.iter().map(|r| r.entries).collect::<Vec<_>>()
        );
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn directories_round_trip(mut rows in proptest::collection::vec(dir(), 1..40), first in 0u32..100) {
            sequence(&mut rows, first);
            let data = encode_dirs(&rows).unwrap();
            prop_assert_eq!(decode_dirs(&data, first).unwrap(), rows.clone());
            projection(&data, first, &rows);
        }
        #[test]
        fn files_round_trip(mut rows in proptest::collection::vec(file(), 1..40)) {
            rows.sort_by(|a,b| (a.dir_id, &a.name).cmp(&(b.dir_id, &b.name)));
            prop_assert_eq!(decode_files(&encode_files(&rows).unwrap()).unwrap(), rows);
        }
        #[test]
        fn links_round_trip(mut rows in proptest::collection::vec(link(), 1..40)) {
            rows.sort_by(|a,b| (a.ino, a.dir_id, &a.name).cmp(&(b.ino, b.dir_id, &b.name)));
            prop_assert_eq!(decode_link_instances(&encode_link_instances(&rows).unwrap()).unwrap(), rows);
        }
    }
    fn is_invalid<T>(r: Result<T>) {
        assert!(matches!(r, Err(StoreError::Invalid(_))));
    }
    fn check_errors(data: &[u8]) {
        for r in [
            decode_dirs(data, 0).map(|_| ()),
            decode_dir_projection(data, 0).map(|_| ()),
            decode_files(data).map(|_| ()),
            decode_link_instances(data).map(|_| ()),
        ] {
            if let Err(e) = r {
                assert!(matches!(e, StoreError::Invalid(_)));
            }
        }
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(3072))]
        #[test]
        fn random_mutations_never_panic(kind in 0..3u8, edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..16), noise in proptest::collection::vec(any::<u8>(), 0..128)) {
            let mut data = match kind {
                0 => encode_dirs(&[ChunkDir::default()]).unwrap(),
                1 => encode_files(&[ChunkFile::default()]).unwrap(),
                _ => encode_link_instances(&[ChunkLinkInstance::default()]).unwrap(),
            };
            for (i, value) in edits { let pos = i % data.len(); data[pos] ^= value; }
            check_errors(&data); check_errors(&noise);
        }
    }
    #[test]
    fn truncated_wrong_version_and_trailing() {
        let blobs = [
            encode_dirs(&[ChunkDir::default()]).unwrap(),
            encode_files(&[ChunkFile::default()]).unwrap(),
            encode_link_instances(&[ChunkLinkInstance::default()]).unwrap(),
        ];
        for (kind, original) in blobs.iter().enumerate() {
            let decode = |data: &[u8]| match kind {
                0 => decode_dirs(data, 0).map(|_| ()),
                1 => decode_files(data).map(|_| ()),
                _ => decode_link_instances(data).map(|_| ()),
            };
            for n in 0..original.len() {
                is_invalid(decode(&original[..n]));
            }
            let mut data = original.clone();
            data[0] = 2;
            is_invalid(decode(&data));
            let mut data = original.clone();
            data.push(0);
            is_invalid(decode(&data));
            let mut data = original.clone();
            data.extend_from_slice(original);
            is_invalid(decode(&data));
            let mut data = original.clone();
            data.splice(1..2, [0xa1, 0]);
            is_invalid(decode(&data));
        }
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(8))]
        #[test]
        fn full_chunk_properties(d in dir(), f in file(), l in link()) {
            let mut dirs = vec![d; 4096]; sequence(&mut dirs, 0);
            let data = encode_dirs(&dirs).unwrap();
            prop_assert_eq!(decode_dirs(&data, 0).unwrap(), dirs.clone());
            projection(&data, 0, &dirs);
            let files = vec![f; MAX_ROWS];
            prop_assert_eq!(decode_files(&encode_files(&files).unwrap()).unwrap(), files);
            let links = vec![l; MAX_ROWS];
            prop_assert_eq!(decode_link_instances(&encode_link_instances(&links).unwrap()).unwrap(), links);
        }
    }
    #[test]
    fn extremes_empty_single_and_full_chunks() {
        for n in [0, 1, 4096, MAX_ROWS] {
            let mut dirs = vec![
                ChunkDir {
                    name: vec![255; 4096],
                    ino: i64::MAX as u64,
                    t_disk: i64::MAX as u64,
                    entries: u32::MAX,
                    errors: u32::MAX,
                    max_entries: u32::MAX,
                    file_count: u32::MAX,
                    mtime: i64::MIN,
                    newest_mtime: i64::MAX,
                    projid: Some(u32::MAX),
                    ..ChunkDir::default()
                };
                n
            ];
            sequence(&mut dirs, 0);
            for r in dirs.iter_mut().skip(1).step_by(2) {
                r.mtime = i64::MAX;
                r.newest_mtime = i64::MIN;
                r.ino = 0;
            }
            let data = encode_dirs(&dirs).unwrap();
            assert_eq!(decode_dirs(&data, 0).unwrap(), dirs);
            projection(&data, 0, &dirs);
            let mut files = vec![
                ChunkFile {
                    name: Some(vec![255; 4096]),
                    ino: i64::MAX as u64,
                    disk: i64::MAX as u64,
                    apparent: i64::MAX as u64,
                    mtime: i64::MIN,
                    nlink: u32::MAX,
                    ..ChunkFile::default()
                };
                n
            ];
            for (i, r) in files.iter_mut().enumerate() {
                r.dir_id = i as u32;
                if i % 2 == 1 {
                    r.name = None;
                    r.mtime = i64::MAX;
                    r.ino = 0;
                }
            }
            assert_eq!(decode_files(&encode_files(&files).unwrap()).unwrap(), files);
            let links: Vec<_> = (0..n)
                .map(|i| ChunkLinkInstance {
                    ino: i64::MAX as u64 - n as u64 + i as u64,
                    dir_id: if i % 2 == 0 { u32::MAX } else { 0 },
                    name: if i % 2 == 0 {
                        None
                    } else {
                        Some(vec![255; 4096])
                    },
                })
                .collect();
            assert_eq!(
                decode_link_instances(&encode_link_instances(&links).unwrap()).unwrap(),
                links
            );
        }
    }
    #[test]
    fn corrupt_columns_and_bounds() {
        let valid = encode_dirs(&[ChunkDir::default()]).unwrap();
        let columns = Columns::open(&valid, 33).unwrap();
        let original: Vec<Vec<u8>> = columns.cursors().iter().map(|c| c.0.to_vec()).collect();
        for column in 0..33 {
            let mut c = original.clone();
            c[column].push(0);
            let blob = frame(c, 3).unwrap();
            is_invalid(decode_dirs(&blob, 0));
            is_invalid(decode_dir_projection(&blob, 0));
            let mut c = original.clone();
            c[column].clear();
            let blob = frame(c, 3).unwrap();
            is_invalid(decode_dirs(&blob, 0));
            is_invalid(decode_dir_projection(&blob, 0));
        }
        for (index, number) in [
            (0, 1u128),
            (1, u128::from(u32::MAX) + 1),
            (2, u128::from(u32::MAX) + 1),
            (7, u128::from(u32::MAX) + 2),
            (8, i64::MAX as u128 + 1),
            (10, u128::from(u32::MAX) + 1),
            (31, u128::from(u32::MAX) + 1),
        ] {
            let mut c = original.clone();
            c[index].clear();
            put(number, &mut c[index]);
            let data = frame(c, 3).unwrap();
            is_invalid(decode_dirs(&data, 0));
            is_invalid(decode_dir_projection(&data, 0));
        }
        let mut bomb = vec![1, 33];
        put(MAX_BYTES as u128 + 1, &mut bomb);
        is_invalid(decode_dirs(&bomb, 0));
        let mut too_many = original.clone();
        for c in &mut too_many {
            *c = c.repeat(MAX_ROWS + 1);
        }
        is_invalid(decode_dirs(&frame(too_many, 3).unwrap(), 0));
        is_invalid(encode_dirs(&vec![ChunkDir::default(); MAX_ROWS + 1]));
        is_invalid(encode_dirs(&[ChunkDir {
            t_disk: u64::MAX,
            ..ChunkDir::default()
        }]));
        is_invalid(encode_files(&[ChunkFile {
            disk: u64::MAX,
            ..ChunkFile::default()
        }]));
        is_invalid(encode_link_instances(&[ChunkLinkInstance {
            ino: u64::MAX,
            ..ChunkLinkInstance::default()
        }]));
        is_invalid(encode_dirs(&[ChunkDir {
            name: vec![0; MAX_NAME + 1],
            ..ChunkDir::default()
        }]));
    }
    #[test]
    fn levels_and_id_boundaries() {
        let row = ChunkDir {
            id: u32::MAX,
            parent_id: Some(0),
            subtree_end: u32::MAX,
            depth: u32::MAX,
            entries: u32::MAX,
            errors: u32::MAX,
            max_entries: u32::MAX,
            file_lo: u32::MAX,
            file_count: u32::MAX,
            ..ChunkDir::default()
        };
        for level in [1, 3, 19] {
            let blob = encode_dirs_with_level(std::slice::from_ref(&row), level).unwrap();
            assert_eq!(decode_dirs(&blob, u32::MAX).unwrap(), vec![row.clone()]);
            assert_eq!(
                decode_files(&encode_files_with_level(&[ChunkFile::default()], level).unwrap())
                    .unwrap(),
                vec![ChunkFile::default()]
            );
            assert_eq!(
                decode_link_instances(
                    &encode_link_instances_with_level(&[ChunkLinkInstance::default()], level)
                        .unwrap()
                )
                .unwrap(),
                vec![ChunkLinkInstance::default()]
            );
        }
        for level in [0, 20] {
            is_invalid(encode_dirs_with_level(&[], level));
            is_invalid(encode_files_with_level(&[], level));
            is_invalid(encode_link_instances_with_level(&[], level));
        }
        is_invalid(encode_dirs(&[ChunkDir {
            id: 1,
            parent_id: None,
            ..ChunkDir::default()
        }]));
        is_invalid(encode_dirs(&[ChunkDir {
            subtree_end: 0,
            ..row
        }]));
        let files = [
            ChunkFile {
                dir_id: 1,
                ..ChunkFile::default()
            },
            ChunkFile::default(),
        ];
        is_invalid(encode_files(&files));
        is_invalid(encode_link_instances(&[
            ChunkLinkInstance {
                ino: 1,
                ..ChunkLinkInstance::default()
            },
            ChunkLinkInstance::default(),
        ]));
    }
    #[test]
    fn reference_wire_columns() {
        let row = ChunkDir {
            id: 100,
            parent_id: Some(98),
            subtree_end: 102,
            depth: 3,
            ino: 7,
            mtime: -10,
            newest_mtime: 11,
            t_disk: 17,
            file_lo: 4,
            file_count: 5,
            ..ChunkDir::default()
        };
        let data = encode_dirs(&[row]).unwrap();
        let columns = Columns::open(&data, 33).unwrap();
        let mut expected = vec![vec![0]; 33];
        for (index, bytes) in [
            (0, vec![2]),
            (1, vec![2]),
            (2, vec![3]),
            (4, vec![14]),
            (5, vec![19]),
            (6, vec![42]),
            (19, vec![17]),
            (31, vec![4]),
            (32, vec![5]),
        ] {
            expected[index] = bytes;
        }
        assert_eq!(
            columns
                .cursors()
                .iter()
                .map(|c| c.0.to_vec())
                .collect::<Vec<_>>(),
            expected
        );
        let data = encode_files(&[ChunkFile {
            dir_id: 5,
            name: None,
            ino: 7,
            disk: 8,
            apparent: 9,
            mtime: -10,
            nlink: 11,
        }])
        .unwrap();
        let columns = Columns::open(&data, 7).unwrap();
        assert_eq!(columns.raw, vec![5, 0, 14, 8, 9, 19, 11]);
        let data = encode_link_instances(&[ChunkLinkInstance {
            ino: 7,
            dir_id: 5,
            name: Some(vec![0, 255]),
        }])
        .unwrap();
        assert_eq!(Columns::open(&data, 3).unwrap().raw, vec![7, 10, 3, 0, 255]);
    }
    #[test]
    fn hash_widths() {
        assert_eq!(
            HashWidth::new(32).unwrap().key(u64::MAX),
            i64::from(u32::MAX)
        );
        assert_eq!(HashWidth::new(64).unwrap().key(u64::MAX), -1);
        assert_eq!(HashWidth::Bits64.bits(), 64);
        is_invalid(HashWidth::new(16));
    }
    #[test]
    fn deterministic_size_sample() {
        let mut rows: Vec<_> = (0..4096)
            .map(|i| ChunkDir {
                name: format!("directory-{i:05}").into_bytes(),
                ino: 1_000_000 + i * 17,
                mtime: 1_700_000_000 - (i as i64 % 365) * 86400,
                newest_mtime: 1_700_000_000,
                dir_disk: 4096,
                dir_apparent: 128,
                entries: (i % 1000) as u32,
                own_files: i % 997,
                own_charged: 4096 + (i % 997) * 8192,
                t_disk: (i % 10000) * 8192,
                t_files: i % 997,
                ..ChunkDir::default()
            })
            .collect();
        sequence(&mut rows, 0);
        let blob = encode_dirs(&rows).unwrap();
        eprintln!(
            "4096 directory sample: {} bytes; {:.4} bytes/row",
            blob.len(),
            blob.len() as f64 / 4096.0
        );
        assert_eq!(decode_dirs(&blob, 0).unwrap(), rows);
    }
}

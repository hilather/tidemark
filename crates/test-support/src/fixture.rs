//! Owned by: W0-C
//! Byte-safe TOML fixtures confined to an owned temporary directory.
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, File, Permissions},
    io::{self, Write},
    os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, PermissionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tidemark_core::units::ByteSize;
/// Maximum materialized entries, including generated entries.
pub const MAX_FIXTURE_ENTRIES: usize = 250_000;
/// Fixture storage. Root and outside are siblings within `dir` and removed on drop.
#[derive(Debug)]
pub struct Fixture {
    /// Temporary owner directory.
    pub dir: TempDir,
    /// Scan root.
    pub root: PathBuf,
    /// Entries deliberately outside the scan root.
    pub outside: PathBuf,
    storage: OwnedStorage,
    modes: Vec<File>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.storage.cleanup(&self.modes) {
            eprintln!("fixture cleanup preserved storage: {error}");
        }
    }
}

// A bounded, fd-relative cleanup shared with the privileged storage helpers.
// TempDir remains available for its public path API, but its destructor is disabled.
#[derive(Debug)]
pub(crate) struct OwnedStorage {
    parent: File,
    owner: File,
    name: OsString,
}

fn open_directory(parent: &File, name: &std::ffi::OsStr, path_only: bool) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags};
    let access = if path_only {
        OFlags::PATH
    } else {
        OFlags::RDONLY
    };
    let flags = access | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match rustix::fs::openat(parent, name, flags | OFlags::NOATIME, Mode::empty()) {
        Ok(fd) => Ok(File::from(fd)),
        Err(rustix::io::Errno::PERM) => Ok(File::from(rustix::fs::openat(
            parent,
            name,
            flags,
            Mode::empty(),
        )?)),
        Err(error) => Err(error.into()),
    }
}

fn identity(file: &File) -> io::Result<(u64, u64)> {
    let meta = file.metadata()?;
    Ok((meta.dev(), meta.ino()))
}

fn verify(parent: &File, name: &std::ffi::OsStr, file: &File) -> io::Result<()> {
    let found = rustix::fs::statat(parent, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)?;
    if (found.st_dev, found.st_ino) != identity(file)? {
        return Err(io::Error::other(
            "directory identity changed; refusing cleanup",
        ));
    }
    Ok(())
}

fn mount_id(file: &File) -> io::Result<u64> {
    use rustix::fs::{statx, AtFlags, StatxFlags};
    match statx(
        file,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::MNT_ID,
    ) {
        Ok(stat) if stat.stx_mask & StatxFlags::MNT_ID.bits() != 0 => return Ok(stat.stx_mnt_id),
        Ok(_) | Err(rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL) => {}
        Err(error) => return Err(error.into()),
    }
    proc_mount_id(file)
}

fn proc_mount_id(file: &File) -> io::Result<u64> {
    use std::{io::Read, os::fd::AsRawFd};
    // Rocky 8's older kernels expose mount IDs through proc fd metadata even
    // when statx lacks STATX_MNT_ID. Read only bounded kernel metadata, never
    // fixture file contents. If proc is unavailable, preserve the fixture.
    let info = File::from(rustix::fs::open(
        format!("/proc/self/fdinfo/{}", file.as_raw_fd()),
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?);
    let mut text = String::new();
    info.take(4096).read_to_string(&mut text)?;
    text.lines()
        .find_map(|line| line.strip_prefix("mnt_id:")?.trim().parse().ok())
        .ok_or_else(|| io::Error::other("mount identity unavailable; preserving fixture"))
}

impl OwnedStorage {
    pub(crate) fn new(dir: &mut TempDir) -> io::Result<Self> {
        // Disable path-based rollback even if acquiring the anchor fails.
        dir.disable_cleanup(true);
        let parent_path = dir
            .path()
            .parent()
            .ok_or_else(|| invalid("missing fixture parent"))?;
        let parent = File::open(parent_path)?;
        let name = dir
            .path()
            .file_name()
            .ok_or_else(|| invalid("missing fixture name"))?
            .to_owned();
        let owner = open_directory(&parent, &name, false)?;
        verify(&parent, &name, &owner)?;
        Ok(Self {
            parent,
            owner,
            name,
        })
    }

    pub(crate) fn verify_owner(&self) -> io::Result<()> {
        verify(&self.parent, &self.name, &self.owner)
    }

    #[cfg(feature = "privileged")]
    pub(crate) fn verify_path(&self, path: &Path) -> io::Result<()> {
        let mut current = File::open("/")?;
        for component in path.components() {
            match component {
                std::path::Component::RootDir => {}
                std::path::Component::Normal(name) => {
                    current = open_directory(&current, name, true)?
                }
                _ => return Err(invalid("non-absolute privileged fixture path")),
            }
        }
        if identity(&current)? != identity(&self.owner)? {
            return Err(io::Error::other("privileged fixture pathname changed"));
        }
        self.verify_owner()
    }

    pub(crate) fn cleanup(&self, modes: &[File]) -> io::Result<()> {
        struct Frame {
            name: OsString,
            file: File,
            entries: rustix::fs::Dir,
        }
        // Both traversal work and retained descriptors have explicit limits.
        const MAX_DEPTH: usize = 1024;
        const MAX_VISITS: usize = MAX_FIXTURE_ENTRIES * 4;
        let mut tracked = std::collections::BTreeMap::new();
        for file in modes {
            tracked.insert(identity(file)?, file);
        }
        self.verify_owner()?;
        let file = self.owner.try_clone()?;
        let entries = rustix::fs::Dir::read_from(&file)?;
        let mut stack = vec![Frame {
            name: self.name.clone(),
            file,
            entries,
        }];
        let device = identity(&self.owner)?.0;
        let mount = mount_id(&self.owner)?;
        let mut visits = 0;
        while !stack.is_empty() {
            // Revalidate the entire ancestry before each mutation: a retained
            // child handle alone would also reach a directory moved outside.
            self.verify_owner()?;
            for i in 1..stack.len() {
                verify(&stack[i - 1].file, &stack[i].name, &stack[i].file)?;
            }
            let i = stack.len() - 1;
            let entry = stack[i].entries.next().transpose()?;
            let Some(entry) = entry else {
                let parent = if i == 0 {
                    &self.parent
                } else {
                    &stack[i - 1].file
                };
                verify(parent, &stack[i].name, &stack[i].file)?;
                rustix::fs::unlinkat(parent, &stack[i].name, rustix::fs::AtFlags::REMOVEDIR)?;
                stack.pop();
                continue;
            };
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            visits += 1;
            if visits > MAX_VISITS {
                return Err(io::Error::other("fixture cleanup visit limit"));
            }
            let name = OsString::from_vec(bytes.to_vec());
            let stat =
                rustix::fs::statat(&stack[i].file, &name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)?;
            if rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Directory
            {
                if stack.len() >= MAX_DEPTH || stat.st_dev != device {
                    return Err(io::Error::other("fixture cleanup depth/device limit"));
                }
                let probe = open_directory(&stack[i].file, &name, true)?;
                verify(&stack[i].file, &name, &probe)?;
                if identity(&probe)? != (stat.st_dev, stat.st_ino) {
                    return Err(io::Error::other("directory changed during cleanup"));
                }
                if mount_id(&probe)? != mount {
                    return Err(io::Error::other("refusing fixture mount point"));
                }
                if let Some(saved) = tracked.get(&identity(&probe)?) {
                    saved.set_permissions(Permissions::from_mode(0o700))?;
                }
                let file = open_directory(&stack[i].file, &name, false)?;
                if identity(&file)? != identity(&probe)? {
                    return Err(io::Error::other("directory changed before descent"));
                }
                let entries = rustix::fs::Dir::read_from(&file)?;
                stack.push(Frame {
                    name,
                    file,
                    entries,
                });
            } else {
                rustix::fs::unlinkat(&stack[i].file, &name, rustix::fs::AtFlags::empty())?;
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    #[serde(default)]
    entry: Vec<Entry>,
    #[serde(default)]
    outside: Vec<Entry>,
    generate: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    kind: String,
    size: Option<ByteSize>,
    sparse: Option<ByteSize>,
    target: Option<String>,
    mode: Option<u32>,
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
/// Decode byte escapes. Only `\xNN` is special; other backslashes are literal.
pub fn decode_bytes(text: &str) -> io::Result<Vec<u8>> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
            let hex = b
                .get(i + 2..i + 4)
                .ok_or_else(|| invalid("short byte escape"))?;
            let digit = |c: u8| {
                (c as char)
                    .to_digit(16)
                    .map(|n| n as u8)
                    .ok_or_else(|| invalid("invalid byte escape"))
            };
            out.push(digit(hex[0])? * 16 + digit(hex[1])?);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    if out.contains(&0) {
        return Err(invalid("NUL in fixture path"));
    }
    Ok(out)
}
fn relative(text: &str) -> io::Result<PathBuf> {
    let b = decode_bytes(text)?;
    if b.is_empty()
        || b.starts_with(b"/")
        || b.split(|&v| v == b'/')
            .any(|c| c.is_empty() || c == b"." || c == b"..")
    {
        return Err(invalid(
            "fixture path must be a nonempty relative path without dot components",
        ));
    }
    Ok(PathBuf::from(OsString::from_vec(b)))
}
fn generated(profile: &str) -> io::Result<Vec<Entry>> {
    let entry = |path: String, kind: &str, size: u64, target: Option<String>| Entry {
        path,
        kind: kind.into(),
        size: Some(ByteSize(size)),
        sparse: None,
        target,
        mode: None,
    };
    let mut entries = vec![];
    match profile {
        "wide_dir" => {
            entries.push(entry("wide".into(), "dir", 0, None));
            for i in 0..120_000 {
                entries.push(entry(format!("wide/f{i:06}"), "file", 0, None));
            }
        }
        "deep" => {
            let mut p = String::new();
            for i in 0..200 {
                if !p.is_empty() {
                    p.push('/');
                }
                p.push_str(&format!("level{i:03}_xxxxxxxx"));
                entries.push(entry(p.clone(), "dir", 0, None));
            }
        }
        "hardlink_web" => {
            for i in 0..50 {
                entries.push(entry(format!("d{i:02}"), "dir", 0, None));
            }
            for set in 2..=50 {
                let source = format!("d00/s{set}");
                entries.push(entry(source.clone(), "file", 4096, None));
                for d in 1..set {
                    entries.push(entry(
                        format!("d{d:02}/s{set}"),
                        "hardlink",
                        0,
                        Some(source.clone()),
                    ));
                }
            }
        }
        _ => return Err(invalid("unknown generation profile")),
    }
    Ok(entries)
}
/// Fallible fixture constructor. Validation rejects symlink parents and path escapes.
pub fn try_materialize(spec: &str) -> io::Result<Fixture> {
    if spec.len() > 32 * 1024 * 1024 {
        return Err(invalid("fixture spec exceeds 32 MiB"));
    }
    let mut spec: Spec = toml::from_str(spec).map_err(|e| invalid(e.to_string()))?;
    if let Some(profile) = &spec.generate {
        spec.entry.extend(generated(profile)?);
    }
    if spec.entry.len() + spec.outside.len() > MAX_FIXTURE_ENTRIES {
        return Err(invalid("fixture entry cap exceeded"));
    }
    let mut dir = crate::private_tempdir()?;
    let storage = OwnedStorage::new(&mut dir)?;
    let root = dir.path().join("r");
    let outside = dir.path().join("outside");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    let mut fixture = Fixture {
        storage,
        dir,
        root,
        outside,
        modes: vec![],
    };
    let mut entries = vec![];
    let mut names = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    let mut nondirs = BTreeSet::new();
    for (list, base) in [
        (&spec.entry, &fixture.root),
        (&spec.outside, &fixture.outside),
    ] {
        for e in list {
            let path = base.join(relative(&e.path)?);
            if !names.insert(path.clone()) {
                return Err(invalid("duplicate fixture entry"));
            }
            if ![
                "file", "dir", "hardlink", "symlink", "fifo", "socket", "char",
            ]
            .contains(&e.kind.as_str())
            {
                return Err(invalid("unknown fixture kind"));
            }
            if e.size.is_some() && e.sparse.is_some() {
                return Err(invalid("size and sparse conflict"));
            }
            if e.mode.is_some_and(|m| m > 0o7777) {
                return Err(invalid("invalid mode"));
            }
            if e.kind == "dir" {
                dirs.insert(path.clone());
            } else {
                nondirs.insert(path.clone());
            }
            let mut parent = path.parent();
            while let Some(p) = parent {
                if p == base {
                    break;
                }
                dirs.insert(p.to_path_buf());
                parent = p.parent();
            }
            entries.push((e, path));
        }
    }
    if dirs.iter().any(|d| nondirs.contains(d)) {
        return Err(invalid("non-directory used as fixture parent"));
    }
    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_by_key(|p| p.components().count());
    for d in dirs {
        fs::create_dir_all(d)?;
    }
    for (e, path) in &entries {
        match e.kind.as_str() {
            "file" => {
                let mut f = File::create(path)?;
                if let Some(size) = e.sparse {
                    f.set_len(size.0)?;
                } else {
                    let mut left = e.size.unwrap_or_default().0;
                    let buf = [0x5a; 64 * 1024];
                    while left > 0 {
                        let n = left.min(buf.len() as u64) as usize;
                        f.write_all(&buf[..n])?;
                        left -= n as u64;
                    }
                }
            }
            "symlink" => {
                let target = decode_bytes(
                    e.target
                        .as_deref()
                        .ok_or_else(|| invalid("symlink needs target"))?,
                )?;
                std::os::unix::fs::symlink(OsString::from_vec(target), path)?;
            }
            "fifo" => rustix::fs::mkfifoat(
                rustix::fs::CWD,
                path,
                rustix::fs::Mode::from_raw_mode(0o600),
            )?,
            "socket" => {
                let _socket = UnixListener::bind(path)?;
            }
            "char" => {
                if rustix::process::geteuid().is_root() {
                    rustix::fs::mknodat(
                        rustix::fs::CWD,
                        path,
                        rustix::fs::FileType::CharacterDevice,
                        rustix::fs::Mode::from_raw_mode(0o600),
                        rustix::fs::makedev(1, 3),
                    )?;
                } else {
                    eprintln!("fixture: skipping device node without root");
                }
            }
            "dir" | "hardlink" => {}
            _ => return Err(invalid("unknown fixture kind")),
        }
    }
    // Resolve forward hardlinks iteratively, without ever following a symlink.
    let mut pending: Vec<_> = entries
        .iter()
        .filter(|(e, _)| e.kind == "hardlink")
        .map(|(e, p)| (*e, p))
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next = vec![];
        for (e, path) in pending {
            let target = fixture.root.join(relative(
                e.target
                    .as_deref()
                    .ok_or_else(|| invalid("hardlink needs target"))?,
            )?);
            // Validate target parents too: lstat on a final component alone would
            // still follow a symlink in an intermediate component.
            let relative_target = target
                .strip_prefix(&fixture.root)
                .map_err(|e| invalid(e.to_string()))?;
            let mut parent = fixture.root.clone();
            let parts: Vec<_> = relative_target.components().collect();
            for part in parts.iter().take(parts.len().saturating_sub(1)) {
                parent.push(part);
                match fs::symlink_metadata(&parent) {
                    Ok(meta) if meta.is_dir() => {}
                    Ok(_) => return Err(invalid("hardlink target has non-directory parent")),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => break,
                    Err(e) => return Err(e),
                }
            }
            // Every parent is a validated directory created above, or absent.
            if let Ok(meta) = fs::symlink_metadata(&target) {
                if !meta.is_file() {
                    return Err(invalid("hardlink target must be regular file"));
                }
                fs::hard_link(target, path)?;
            } else {
                next.push((e, path));
            }
        }
        if next.len() == before {
            return Err(invalid("missing or cyclic hardlink target"));
        }
        pending = next;
    }
    // Resolve delayed allocation for the whole fixture (including outside links)
    // before tests measure stat or du, without one writeback per regular file.
    let root = open_directory(&fixture.storage.owner, std::ffi::OsStr::new("r"), false)?;
    // Apply directory modes last so descendants can be populated. Retain handles for identity-based cleanup.
    let mut modes: Vec<_> = entries
        .iter()
        .filter(|(e, _)| e.kind == "dir" && e.mode.is_some())
        .collect();
    modes.sort_by_key(|(_, p)| p.components().count());
    fixture.modes = modes
        .iter()
        .map(|(_, p)| File::open(p))
        .collect::<io::Result<_>>()?;
    for (e, path) in modes.into_iter().rev() {
        fs::set_permissions(path, Permissions::from_mode(e.mode.unwrap_or(0o755)))?;
    }
    rustix::fs::syncfs(&root)?;
    Ok(fixture)
}
/// Materialize a test fixture, panicking with context on invalid input or I/O failure.
pub fn materialize(spec: &str) -> Fixture {
    try_materialize(spec).unwrap_or_else(|e| panic!("cannot materialize fixture: {e}"))
}
/// Read a fixture TOML file and materialize it into owned temporary storage.
pub fn materialize_file(path: impl AsRef<Path>) -> Fixture {
    let spec = fs::read_to_string(path.as_ref())
        .unwrap_or_else(|e| panic!("cannot read fixture spec: {e}"));
    materialize(&spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn substitution(intermediate: bool, outside: bool) {
        let section = if outside { "outside" } else { "entry" };
        let fixture = materialize(&format!(
            "[[{section}]]\npath = 'parent'\nkind = 'dir'\nmode = 320\n[[{section}]]\npath = 'parent/child'\nkind = 'dir'\nmode = 320\n"
        ));
        let owner_path = fixture.dir.path().to_owned();
        let base = if outside {
            &fixture.outside
        } else {
            &fixture.root
        };
        let victim = crate::private_tempdir().unwrap();
        let victim_child = victim.path().join("child");
        fs::create_dir(&victim_child).unwrap();
        fs::set_permissions(victim.path(), Permissions::from_mode(0o500)).unwrap();
        fs::set_permissions(&victim_child, Permissions::from_mode(0o500)).unwrap();
        fs::set_permissions(base.join("parent"), Permissions::from_mode(0o700)).unwrap();
        let replaced = base.join(if intermediate {
            "parent"
        } else {
            "parent/child"
        });
        fs::set_permissions(&replaced, Permissions::from_mode(0o700)).unwrap();
        fs::rename(&replaced, base.join("saved")).unwrap();
        std::os::unix::fs::symlink(victim.path(), &replaced).unwrap();
        drop(fixture);
        assert_eq!(fs::metadata(victim.path()).unwrap().mode() & 0o7777, 0o500);
        assert_eq!(fs::metadata(&victim_child).unwrap().mode() & 0o7777, 0o500);
        assert!(!owner_path.exists());
    }

    #[test]
    fn proc_mount_identity_matches_statx() {
        let dir = crate::private_tempdir().unwrap();
        let file = File::open(dir.path()).unwrap();
        assert_eq!(proc_mount_id(&file).unwrap(), mount_id(&file).unwrap());
    }

    #[test]
    fn cleanup_preserves_replaced_owner() {
        let sandbox = crate::private_tempdir().unwrap();
        let mut dir = tempfile::tempdir_in(sandbox.path()).unwrap();
        let storage = OwnedStorage::new(&mut dir).unwrap();
        fs::rename(dir.path(), sandbox.path().join("moved")).unwrap();
        fs::create_dir(dir.path()).unwrap();
        fs::write(dir.path().join("sentinel"), b"safe").unwrap();
        assert!(storage.cleanup(&[]).is_err());
        assert_eq!(fs::read(dir.path().join("sentinel")).unwrap(), b"safe");
        assert!(sandbox.path().join("moved").exists());
    }

    #[test]
    fn cleanup_survives_parent_symlink_substitution() {
        let sandbox = crate::private_tempdir().unwrap();
        let parent = sandbox.path().join("parent");
        fs::create_dir(&parent).unwrap();
        // Construct under a private parent without changing process-wide TMPDIR.
        let mut dir = tempfile::tempdir_in(&parent).unwrap();
        let storage = OwnedStorage::new(&mut dir).unwrap();
        let root = dir.path().join("r");
        let outside = dir.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let name = dir.path().file_name().unwrap().to_owned();
        let fixture = Fixture {
            dir,
            root,
            outside,
            storage,
            modes: vec![],
        };
        let moved = sandbox.path().join("moved");
        fs::rename(&parent, &moved).unwrap();
        let victim = crate::private_tempdir().unwrap();
        let target = victim.path().join(&name);
        fs::create_dir(&target).unwrap();
        fs::write(target.join("sentinel"), b"safe").unwrap();
        std::os::unix::fs::symlink(victim.path(), &parent).unwrap();
        drop(fixture);
        assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"safe");
        assert!(!moved.join(name).exists());
    }

    #[test]
    fn cleanup_finds_internal_renames_by_identity() {
        let fixture = materialize("[[entry]]\npath = 'parent'\nkind = 'dir'\nmode = 0\n[[entry]]\npath = 'parent/child'\nkind = 'dir'\nmode = 0\n");
        let owner = fixture.dir.path().to_owned();
        // Linux requires write access on the directory while updating its parent.
        fixture.modes[0]
            .set_permissions(Permissions::from_mode(0o700))
            .unwrap();
        fs::rename(fixture.root.join("parent"), fixture.outside.join("renamed")).unwrap();
        fixture.modes[0]
            .set_permissions(Permissions::from_mode(0o0))
            .unwrap();
        drop(fixture);
        assert!(!owner.exists());
    }

    #[test]
    fn cleanup_does_not_restore_directory_moved_outside() {
        let fixture = materialize("[[entry]]\npath = 'tracked'\nkind = 'dir'\nmode = 0\n");
        let victim = crate::private_tempdir().unwrap();
        let destination = victim.path().join("moved");
        fixture.modes[0]
            .set_permissions(Permissions::from_mode(0o700))
            .unwrap();
        fs::rename(fixture.root.join("tracked"), &destination).unwrap();
        fixture.modes[0]
            .set_permissions(Permissions::from_mode(0o0))
            .unwrap();
        drop(fixture);
        assert_eq!(fs::metadata(&destination).unwrap().mode() & 0o7777, 0);
        fs::set_permissions(destination, Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn cleanup_skips_final_symlink() {
        substitution(false, false);
    }
    #[test]
    fn cleanup_skips_intermediate_symlink() {
        substitution(true, false);
    }
    #[test]
    fn outside_cleanup_skips_symlinks() {
        substitution(false, true);
        substitution(true, true);
    }
    #[test]
    fn cleanup_skips_missing_and_replaced_entries() {
        for replacement in ["missing", "file", "directory"] {
            let fixture = materialize("[[entry]]\npath = 'tracked'\nkind = 'dir'\nmode = 320\n");
            let path = fixture.root.join("tracked");
            fs::remove_dir(&path).unwrap();
            match replacement {
                "file" => {
                    fs::write(&path, b"safe").unwrap();
                }
                "directory" => {
                    fs::create_dir(&path).unwrap();
                }
                _ => {}
            }
            if replacement != "missing" {
                fs::set_permissions(&path, Permissions::from_mode(0o500)).unwrap();
                // Retain a handle so its mode can be inspected after removal.
                let handle = File::open(&path).unwrap();
                drop(fixture);
                assert_eq!(handle.metadata().unwrap().mode() & 0o7777, 0o500);
            } else {
                drop(fixture);
            }
        }
    }

    #[test]
    fn random_tree_removal_does_not_follow_symlinks() {
        let fixture = crate::random::random_tree(42, crate::random::RandomTreeParams::default());
        let path = fixture.dir.path().to_owned();
        let victim = crate::private_tempdir().unwrap();
        fs::write(victim.path().join("sentinel"), b"safe").unwrap();
        std::os::unix::fs::symlink(victim.path(), fixture.root.join("victim")).unwrap();
        drop(fixture);
        assert!(victim.path().join("sentinel").exists());
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_restores_inaccessible_parents_first() {
        let fixture = materialize("[[entry]]\npath = 'parent'\nkind = 'dir'\nmode = 0\n[[entry]]\npath = 'parent/child'\nkind = 'dir'\nmode = 0\n");
        let path = fixture.dir.path().to_owned();
        drop(fixture);
        assert!(!path.exists());
    }
}

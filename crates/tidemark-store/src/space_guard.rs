//! Owned by: W1-E
//! Conservative filesystem-space checks for Tidemark's own state.
use std::{io, path::Path};
use tidemark_core::config::RetentionConfig;
/// Space-policy decision, without performing pruning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceVerdict {
    /// The estimated write fits the policy.
    Ok,
    /// Writing would violate the policy.
    Refuse {
        /// Explanation for operators and the state_space alert.
        reason: String,
    },
}
/// Byte totals measured on the filesystem containing the state directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsTotals {
    /// Total filesystem bytes.
    pub size: u64,
    /// Bytes available to an unprivileged writer (conservative for root).
    pub avail: u64,
}
fn totals(path: &Path) -> io::Result<FsTotals> {
    let verified = crate::state_dir::StateDir::open(path, false)?;
    let stat = rustix::fs::fstatvfs(verified.fd())?;
    let size = stat
        .f_blocks
        .checked_mul(stat.f_frsize)
        .ok_or_else(|| io::Error::other("filesystem size overflow"))?;
    let avail = stat
        .f_bavail
        .checked_mul(stat.f_frsize)
        .ok_or_else(|| io::Error::other("filesystem available size overflow"))?;
    Ok(FsTotals { size, avail })
}
pub(crate) fn mount_id(fd: impl std::os::fd::AsFd) -> io::Result<u64> {
    use std::{io::Read, os::fd::AsRawFd};
    // fdinfo exposes metadata, not target contents. mnt_id is available on EL8.
    let file = std::fs::File::open(format!("/proc/self/fdinfo/{}", fd.as_fd().as_raw_fd()))?;
    let mut bytes = Vec::new();
    file.take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err(io::Error::other("oversized directory fdinfo"));
    }
    for line in bytes.split(|&b| b == b'\n') {
        if let Some(id) = line.strip_prefix(b"mnt_id:") {
            return std::str::from_utf8(id)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .ok_or_else(|| io::Error::other("invalid fdinfo mount id"));
        }
    }
    Err(io::Error::other("fdinfo has no mount id"))
}
fn directory(
    fd: impl std::os::fd::AsFd,
    path: impl rustix::path::Arg + Copy,
) -> io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{openat, Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match openat(&fd, path, flags | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => Ok(openat(fd, path, flags, Mode::empty())?),
        value => Ok(value?),
    }
}
#[cfg(test)]
type ChildOpenHook = Box<dyn FnMut(&[u8]) -> Option<rustix::io::Errno>>;
#[cfg(test)]
thread_local! {
    static BEFORE_CHILD_OPEN: std::cell::RefCell<Option<ChildOpenHook>> =
        const { std::cell::RefCell::new(None) };
}
/// Runs after a directory entry is yielded and before `openat`.
///
/// `Some` replaces the `openat` result so a test can force an errno. `None`
/// performs the real open.
#[cfg(test)]
fn before_child_open(name: &[u8]) -> Option<rustix::io::Errno> {
    BEFORE_CHILD_OPEN.with(|hook| hook.borrow_mut().as_mut().and_then(|hook| hook(name)))
}
fn state_usage(path: &Path) -> io::Result<u64> {
    use rustix::fs::{self, Dir, FileType, Mode, OFlags};
    // Iterative DFS has at most 4096 retained directory fds and one entry fd.
    const MAX_DEPTH: usize = 4096;
    let verified = crate::state_dir::StateDir::open(path, false)?;
    let root = rustix::io::dup(verified.fd())?;
    let root_mount = mount_id(&root)?;
    let root_stat = fs::fstat(&root)?;
    let mut used = u64::try_from(root_stat.st_blocks)
        .ok()
        .and_then(|n| n.checked_mul(512))
        .ok_or_else(|| io::Error::other("state allocation overflow"))?;
    let mut stack = vec![Dir::new(root)?];
    while let Some(iter) = stack.last_mut() {
        let entry = match iter.next() {
            None => {
                stack.pop();
                continue;
            }
            Some(entry) => entry?,
        };
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        // O_PATH never reads contents; NOFOLLOW binds this fd to the entry itself.
        // The API socket and its lock are unlinked while a scan is still measuring
        // state. A name already returned by getdents can be gone before open; that
        // leaf is not allocated space. Every other errno, including EACCES, still
        // fails the measurement.
        #[cfg(test)]
        let injected = before_child_open(name.to_bytes());
        #[cfg(not(test))]
        let injected: Option<rustix::io::Errno> = None;
        let opened = match injected {
            Some(errno) => Err(errno),
            None => fs::openat(
                iter.fd()?,
                name,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            ),
        };
        let child = match opened {
            Err(rustix::io::Errno::NOENT) => continue,
            other => other?,
        };
        if mount_id(&child)? != root_mount {
            continue;
        }
        let stat = fs::fstat(&child)?;
        if stat.st_dev != root_stat.st_dev {
            continue;
        }
        let bytes = u64::try_from(stat.st_blocks)
            .ok()
            .and_then(|n| n.checked_mul(512))
            .ok_or_else(|| io::Error::other("state allocation overflow"))?;
        used = used
            .checked_add(bytes)
            .ok_or_else(|| io::Error::other("state allocation overflow"))?;
        if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
            if stat.st_uid != rustix::process::geteuid().as_raw() || stat.st_mode & 0o077 != 0 {
                return Err(io::Error::other(format!(
                    "state directory {}: child {} is not private and owned",
                    tidemark_core::path::display_lossy_bytes(path.as_os_str().as_encoded_bytes()),
                    tidemark_core::path::display_lossy_bytes(name.to_bytes())
                )));
            }
            if stack.len() >= MAX_DEPTH {
                return Err(io::Error::other("state directory depth exceeds 4096"));
            }
            // Open through the already bound entry fd, so a replacement race is harmless.
            stack.push(Dir::new(directory(&child, ".")?)?);
        }
    }
    Ok(used)
}
/// Measure state allocation without following symlinks, then apply space policy.
///
/// Pruning is the caller's responsibility; this function never deletes snapshots.
pub fn space_check(
    state_dir: &Path,
    estimate: u64,
    policy: &RetentionConfig,
) -> io::Result<SpaceVerdict> {
    let fs = totals(state_dir)?;
    let used = state_usage(state_dir)?;
    Ok(space_check_with(|| Ok(fs), used, estimate, policy))
}
/// Apply the state cap and max(percent floor, 2 GiB) free-space floor.
pub fn space_check_with(
    statvfs: impl Fn() -> io::Result<FsTotals>,
    used_by_state: u64,
    estimate: u64,
    policy: &RetentionConfig,
) -> SpaceVerdict {
    let refuse = |reason: &str| SpaceVerdict::Refuse {
        reason: reason.into(),
    };
    if !policy.min_free_pct.is_finite() || !(0.0..=100.0).contains(&policy.min_free_pct) {
        return refuse("invalid minimum free percentage");
    }
    let stat = match statvfs() {
        Ok(s) => s,
        Err(e) => {
            return SpaceVerdict::Refuse {
                reason: format!("cannot measure state filesystem: {e}"),
            }
        }
    };
    if used_by_state
        .checked_add(estimate)
        .is_none_or(|n| n > policy.max_state_size.0)
    {
        return refuse("maximum state size would be exceeded");
    }
    // Decode the nonnegative finite f64 as an exact binary rational.
    let bits = policy.min_free_pct.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let mantissa = (bits & ((1u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1u64 << 52 };
    let shift = if exponent == 0 { 1074 } else { 1075 - exponent };
    // u64 * 53-bit mantissa fits u128; checked operations retain conservative failure.
    let numerator = u128::from(stat.size).checked_mul(u128::from(mantissa));
    let percentage = match numerator {
        Some(0) => 0,
        Some(n) if shift >= 121 => u128::from(n != 0),
        Some(n) => {
            let Some(denominator) = (1u128 << shift).checked_mul(100) else {
                return refuse("free-space floor overflow");
            };
            n / denominator + u128::from(n % denominator != 0)
        }
        None => return refuse("free-space floor overflow"),
    };
    let Ok(floor) = u64::try_from(percentage.max(2 << 30)) else {
        return refuse("free-space floor overflow");
    };
    if stat.avail.checked_sub(estimate).is_none_or(|n| n < floor) {
        return refuse("state filesystem free-space floor would be crossed");
    }
    SpaceVerdict::Ok
}

/// Check an inflate against `policy` and the exact decoded size.
///
/// `policy.min_free_pct` and `policy.max_state_size` are the caller's configured
/// retention floor. The default policy is only for callers that have no config.
pub fn inflate_check(
    cold: &Path,
    length: u64,
    policy: &RetentionConfig,
) -> Result<(), crate::StoreError> {
    let root = crate::state_dir::file_parent(cold)?;
    let state =
        if root.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("snapshots")) {
            root.parent().and_then(Path::parent).unwrap_or(&root)
        } else {
            &root
        };
    match space_check(state, length, policy)? {
        SpaceVerdict::Ok => Ok(()),
        SpaceVerdict::Refuse { reason } => Err(crate::StoreError::Space(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[test]
    fn state_usage_counts_allocation_without_following_symlinks() {
        let own = ::test_support::private_tempdir().unwrap();
        let outside = ::test_support::private_tempdir().unwrap();
        let data = own.path().join("data");
        let child = own.path().join("child");
        ::test_support::private_dir(&child).unwrap();
        std::fs::write(&data, vec![1; 8192]).unwrap();
        std::fs::write(outside.path().join("unrelated"), vec![1; 1024 * 1024]).unwrap();
        let link = child.join("link");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let expected = [own.path(), &data, &child, &link]
            .into_iter()
            .map(|p| std::fs::symlink_metadata(p).unwrap().blocks() * 512)
            .sum::<u64>();
        assert_eq!(state_usage(own.path()).unwrap(), expected);
        let root_link = own.path().join("root-link");
        std::os::unix::fs::symlink(&child, &root_link).unwrap();
        assert!(state_usage(&root_link).is_err());
    }
    struct ClearChildOpenHook;
    impl Drop for ClearChildOpenHook {
        fn drop(&mut self) {
            BEFORE_CHILD_OPEN.with(|hook| *hook.borrow_mut() = None);
        }
    }
    fn install_child_open_hook(hook: ChildOpenHook) -> ClearChildOpenHook {
        BEFORE_CHILD_OPEN.with(|slot| *slot.borrow_mut() = Some(hook));
        ClearChildOpenHook
    }

    /// The scan maps every `space_check` I/O error to a refusal and reports
    /// `ScanError::NoSpace`. A missing name is not that error.
    #[test]
    fn vanished_leaf_between_readdir_and_openat_is_skipped_not_nospace() {
        let own = ::test_support::private_tempdir().unwrap();
        let socket = own.path().join("api.sock");
        std::fs::write(&socket, vec![1; 8192]).unwrap();
        let with_socket = state_usage(own.path()).unwrap();
        let root = own.path().to_owned();
        let _clear = install_child_open_hook(Box::new(move |name| {
            if name == b"api.sock" {
                std::fs::remove_file(root.join("api.sock")).unwrap();
            }
            None
        }));

        let raced = state_usage(own.path()).unwrap();
        let gone = state_usage(own.path()).unwrap();
        assert!(with_socket > gone);
        assert_eq!(raced, gone);

        std::fs::write(&socket, vec![1; 8192]).unwrap();
        let measured =
            space_check(own.path(), 0, &RetentionConfig::default()).unwrap_or_else(|error| {
                panic!("leaf removed between readdir and openat reported as NoSpace: {error}")
            });
        assert!(
            !matches!(
                &measured,
                SpaceVerdict::Refuse { reason }
                    if reason.contains("No such file or directory") || reason.contains("os error 2")
            ),
            "vanished leaf reported as NoSpace: {measured:?}"
        );
        assert!(!socket.exists(), "openat ran before the leaf was removed");
        assert_eq!(
            space_check_with(
                || {
                    Ok(FsTotals {
                        size: 64 << 30,
                        avail: 32 << 30,
                    })
                },
                raced,
                0,
                &RetentionConfig::default(),
            ),
            SpaceVerdict::Ok,
            "skipped leaf must not be reported as NoSpace"
        );
    }

    #[test]
    fn child_open_eacces_propagates() {
        let own = ::test_support::private_tempdir().unwrap();
        let socket = own.path().join("api.sock");
        std::fs::write(&socket, vec![1; 8192]).unwrap();
        let _clear = install_child_open_hook(Box::new(|name| {
            if name == b"api.sock" {
                Some(rustix::io::Errno::ACCESS)
            } else {
                None
            }
        }));

        let usage = state_usage(own.path()).unwrap_err();
        assert_eq!(
            usage.raw_os_error(),
            Some(rustix::io::Errno::ACCESS.raw_os_error())
        );
        let checked = space_check(own.path(), 0, &RetentionConfig::default()).unwrap_err();
        assert_eq!(
            checked.raw_os_error(),
            Some(rustix::io::Errno::ACCESS.raw_os_error())
        );
        assert!(
            socket.exists(),
            "EACCES must not be treated as a vanished leaf"
        );
    }
}

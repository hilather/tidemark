//! Owned by: W1-E
//! Conservative filesystem-space checks for Tidemark's own state.
use std::{io, path::Path};
use tidemark_core::config::RetentionConfig;
/// Why a state-space check refused a write.
///
/// Plain `top` soft-skips only [`SpaceRefusal::Floor`]. A failure to read
/// filesystem totals or measure state usage is [`std::io::Error`], not a variant
/// here, and becomes [`crate::StoreError::Io`] for callers of [`inflate_check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceRefusal {
    /// Free space would fall below the effective floor in `bytes`.
    Floor {
        /// Effective floor: `max(min_free_pct of filesystem size, 2 GiB)`.
        bytes: u64,
        /// The 2 GiB constant determined `bytes` (the percentage was not higher).
        minimum_applied: bool,
    },
    /// `used + estimate` would exceed `max_state_size`.
    MaxStateSize,
    /// The percentage floor could not be computed in range.
    Overflow,
    /// `min_free_pct` is not a finite value in `0..=100`.
    InvalidPolicy,
}
/// Space-policy decision, without performing pruning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceVerdict {
    /// The estimated write fits the policy.
    Ok,
    /// Writing would violate the policy.
    Refuse {
        /// Which rule refused the write.
        ///
        /// `None` is a caller-supplied refusal, not a classification from
        /// [`space_check_with`].
        kind: Option<SpaceRefusal>,
        /// Explanation for operators and the state_space alert.
        reason: String,
    },
}
impl SpaceVerdict {
    /// A refusal that did not come from [`space_check_with`].
    pub fn other(reason: impl Into<String>) -> Self {
        Self::Refuse {
            kind: None,
            reason: reason.into(),
        }
    }
}
fn refuse(kind: SpaceRefusal, reason: &str) -> io::Result<SpaceVerdict> {
    Ok(SpaceVerdict::Refuse {
        kind: Some(kind),
        reason: reason.to_owned(),
    })
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
type ErrnoHook = Box<dyn FnMut(&[u8]) -> Option<rustix::io::Errno>>;
#[cfg(test)]
thread_local! {
    static BEFORE_CHILD_OPEN: std::cell::RefCell<Option<ErrnoHook>> =
        const { std::cell::RefCell::new(None) };
    static BEFORE_DIR_LIST: std::cell::RefCell<Option<ErrnoHook>> =
        const { std::cell::RefCell::new(None) };
    static ON_READDIR: std::cell::RefCell<Option<ErrnoHook>> =
        const { std::cell::RefCell::new(None) };
    static DIR_STACK: std::cell::RefCell<Vec<Vec<u8>>> = const { std::cell::RefCell::new(Vec::new()) };
}
/// Runs after a directory entry is yielded and before `openat`.
///
/// `Some` replaces the `openat` result so a test can force an errno. `None`
/// performs the real open.
#[cfg(test)]
fn before_child_open(name: &[u8]) -> Option<rustix::io::Errno> {
    BEFORE_CHILD_OPEN.with(|hook| hook.borrow_mut().as_mut().and_then(|hook| hook(name)))
}
/// Runs after a directory's `O_PATH` open and before it is opened for listing.
#[cfg(test)]
fn before_directory_list(name: &[u8]) -> Option<rustix::io::Errno> {
    BEFORE_DIR_LIST.with(|hook| hook.borrow_mut().as_mut().and_then(|hook| hook(name)))
}
/// Replaces the next `getdents` result while `name` is the directory being listed.
#[cfg(test)]
fn readdir_fault() -> Option<rustix::io::Errno> {
    let name = DIR_STACK
        .with(|stack| stack.borrow().last().cloned())
        .unwrap_or_default();
    ON_READDIR.with(|hook| hook.borrow_mut().as_mut().and_then(|hook| hook(&name)))
}
#[cfg(test)]
fn enter_dir(name: &[u8]) {
    DIR_STACK.with(|stack| stack.borrow_mut().push(name.to_vec()));
}
#[cfg(test)]
fn leave_dir() {
    DIR_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}
fn is_noent(error: &io::Error) -> bool {
    error.raw_os_error() == Some(rustix::io::Errno::NOENT.raw_os_error())
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
    #[cfg(test)]
    {
        DIR_STACK.with(|stack| stack.borrow_mut().clear());
        enter_dir(b"");
    }
    let mut stack = vec![Dir::new(root)?];
    while let Some(iter) = stack.last_mut() {
        // ENOENT while listing means this directory was removed. Drop the
        // subtree. Every other errno still fails the measurement.
        #[cfg(test)]
        let injected_read = readdir_fault();
        #[cfg(not(test))]
        let injected_read: Option<rustix::io::Errno> = None;
        let read = match injected_read {
            Some(errno) => Some(Err(errno)),
            None => iter.next(),
        };
        let entry = match read {
            None => {
                stack.pop();
                #[cfg(test)]
                leave_dir();
                continue;
            }
            Some(Ok(entry)) => entry,
            Some(Err(rustix::io::Errno::NOENT)) => {
                stack.pop();
                #[cfg(test)]
                leave_dir();
                continue;
            }
            Some(Err(errno)) => return Err(errno.into()),
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
            // The directory can be removed after the O_PATH open and before this
            // listing open. ENOENT means the subtree is gone.
            #[cfg(test)]
            let list_fault = before_directory_list(name.to_bytes());
            #[cfg(not(test))]
            let list_fault: Option<rustix::io::Errno> = None;
            match list_fault {
                Some(rustix::io::Errno::NOENT) => continue,
                Some(errno) => return Err(errno.into()),
                None => {}
            }
            let listed = match directory(&child, ".") {
                Err(error) if is_noent(&error) => continue,
                other => other?,
            };
            #[cfg(test)]
            enter_dir(name.to_bytes());
            stack.push(Dir::new(listed)?);
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
    space_check_with(|| Ok(fs), used, estimate, policy)
}
/// Mandatory free bytes. `2 << 30` is 2 GiB.
const MANDATORY_FREE_BYTES: u64 = 2 << 30;
const OVERFLOW_REASON: &str = "free-space floor overflow";

/// `size * mantissa`, or [`SpaceRefusal::Overflow`] when the product does not fit.
fn scaled_numerator(size: u128, mantissa: u128) -> Result<u128, SpaceRefusal> {
    size.checked_mul(mantissa).ok_or(SpaceRefusal::Overflow)
}
/// `2^shift * 100`, or [`SpaceRefusal::Overflow`] when the product does not fit.
///
/// Valid percentages take the `shift >= 121` shortcut in [`percentage_bytes`]
/// before this multiply. The failure kind stays [`SpaceRefusal::Overflow`].
fn percent_denominator(shift: u32) -> Result<u128, SpaceRefusal> {
    (1u128 << shift)
        .checked_mul(100)
        .ok_or(SpaceRefusal::Overflow)
}
/// Ceil of `size * min_free_pct / 100` as an exact binary rational.
fn percentage_bytes(size: u64, min_free_pct: f64) -> Result<u128, SpaceRefusal> {
    // Decode the nonnegative finite f64 as an exact binary rational.
    let bits = min_free_pct.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let mantissa = (bits & ((1u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1u64 << 52 };
    let shift = if exponent == 0 { 1074 } else { 1075 - exponent };
    // u64 * 53-bit mantissa fits u128; checked operations retain conservative failure.
    let numerator = scaled_numerator(u128::from(size), u128::from(mantissa))?;
    if numerator == 0 {
        return Ok(0);
    }
    if shift >= 121 {
        return Ok(u128::from(numerator != 0));
    }
    let denominator = percent_denominator(shift)?;
    Ok(numerator / denominator + u128::from(numerator % denominator != 0))
}
/// `max(percentage, 2 GiB)`, and whether the 2 GiB constant set that value.
fn apply_mandatory(percentage: u128) -> Result<(u64, bool), SpaceRefusal> {
    let mandatory = u128::from(MANDATORY_FREE_BYTES);
    let bytes = u64::try_from(percentage.max(mandatory)).map_err(|_| SpaceRefusal::Overflow)?;
    Ok((bytes, percentage <= mandatory))
}
/// Apply the state cap and max(percent floor, 2 GiB) free-space floor.
///
/// A failure to read filesystem totals is returned as an error. Policy
/// violations are [`SpaceVerdict::Refuse`].
pub fn space_check_with(
    statvfs: impl Fn() -> io::Result<FsTotals>,
    used_by_state: u64,
    estimate: u64,
    policy: &RetentionConfig,
) -> io::Result<SpaceVerdict> {
    if !policy.min_free_pct.is_finite() || !(0.0..=100.0).contains(&policy.min_free_pct) {
        return refuse(
            SpaceRefusal::InvalidPolicy,
            "invalid minimum free percentage",
        );
    }
    let stat = statvfs()?;
    if used_by_state
        .checked_add(estimate)
        .is_none_or(|n| n > policy.max_state_size.0)
    {
        return refuse(
            SpaceRefusal::MaxStateSize,
            "maximum state size would be exceeded",
        );
    }
    // The next three failures are the historical "free-space floor overflow"
    // sites (numerator, denominator, and floor width). They are Overflow on
    // purpose, so a caller can hard-fail them instead of treating the text
    // as a free-space floor skip.
    let percentage = match percentage_bytes(stat.size, policy.min_free_pct) {
        Ok(percentage) => percentage,
        Err(_) => return refuse(SpaceRefusal::Overflow, OVERFLOW_REASON),
    };
    let (floor, minimum_applied) = match apply_mandatory(percentage) {
        Ok(floor) => floor,
        Err(_) => return refuse(SpaceRefusal::Overflow, OVERFLOW_REASON),
    };
    if stat.avail.checked_sub(estimate).is_none_or(|n| n < floor) {
        return Ok(SpaceVerdict::Refuse {
            kind: Some(SpaceRefusal::Floor {
                bytes: floor,
                minimum_applied,
            }),
            reason: "state filesystem free-space floor would be crossed".to_owned(),
        });
    }
    Ok(SpaceVerdict::Ok)
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
        SpaceVerdict::Refuse { kind, reason } => Err(crate::StoreError::space(kind, reason)),
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
    struct ClearHooks;
    impl Drop for ClearHooks {
        fn drop(&mut self) {
            BEFORE_CHILD_OPEN.with(|hook| *hook.borrow_mut() = None);
            BEFORE_DIR_LIST.with(|hook| *hook.borrow_mut() = None);
            ON_READDIR.with(|hook| *hook.borrow_mut() = None);
            DIR_STACK.with(|stack| stack.borrow_mut().clear());
        }
    }
    fn install_child_open_hook(hook: ErrnoHook) -> ClearHooks {
        BEFORE_CHILD_OPEN.with(|slot| *slot.borrow_mut() = Some(hook));
        ClearHooks
    }
    fn install_dir_list_hook(hook: ErrnoHook) -> ClearHooks {
        BEFORE_DIR_LIST.with(|slot| *slot.borrow_mut() = Some(hook));
        ClearHooks
    }
    fn install_readdir_hook(hook: ErrnoHook) -> ClearHooks {
        ON_READDIR.with(|slot| *slot.borrow_mut() = Some(hook));
        ClearHooks
    }
    fn accepted(usage: u64) -> SpaceVerdict {
        space_check_with(
            || {
                Ok(FsTotals {
                    size: 64 << 30,
                    avail: 32 << 30,
                })
            },
            usage,
            0,
            &RetentionConfig::default(),
        )
        .unwrap()
    }

    /// A missing name is skipped. Only a space refusal becomes `ScanError::NoSpace`;
    /// an I/O error from this measurement stays an I/O error.
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
                SpaceVerdict::Refuse { reason, .. }
                    if reason.contains("No such file or directory") || reason.contains("os error 2")
            ),
            "vanished leaf reported as NoSpace: {measured:?}"
        );
        assert!(!socket.exists(), "openat ran before the leaf was removed");
        assert_eq!(
            accepted(raced),
            SpaceVerdict::Ok,
            "skipped leaf must not be reported as NoSpace"
        );
    }

    #[test]
    fn subdirectory_removed_before_listing_is_skipped() {
        let own = ::test_support::private_tempdir().unwrap();
        let sub = own.path().join("sub");
        ::test_support::private_dir(&sub).unwrap();
        let leaf = sub.join("leaf");
        std::fs::write(&leaf, vec![1; 8192]).unwrap();
        let with_leaf = state_usage(own.path()).unwrap();
        let root = own.path().to_owned();
        let _clear = install_dir_list_hook(Box::new(move |name| {
            if name == b"sub" {
                std::fs::remove_file(root.join("sub/leaf")).unwrap();
                std::fs::remove_dir(root.join("sub")).unwrap();
            }
            None
        }));
        let raced = state_usage(own.path()).unwrap();
        assert!(with_leaf > raced);
        assert!(
            !sub.exists(),
            "listing open ran before the directory was removed"
        );
        ::test_support::private_dir(&sub).unwrap();
        std::fs::write(&leaf, vec![1; 8192]).unwrap();
        space_check(own.path(), 0, &RetentionConfig::default()).unwrap_or_else(|error| {
            panic!("directory removed before listing reported as NoSpace: {error}")
        });
        assert_eq!(accepted(raced), SpaceVerdict::Ok);
    }

    #[test]
    fn directory_removed_during_listing_is_skipped() {
        let own = ::test_support::private_tempdir().unwrap();
        let sub = own.path().join("sub");
        ::test_support::private_dir(&sub).unwrap();
        std::fs::write(sub.join("a"), vec![1; 8192]).unwrap();
        let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = reads.clone();
        let _clear = install_readdir_hook(Box::new(move |name| {
            if name == b"sub" {
                let seen = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if seen >= 1 {
                    return Some(rustix::io::Errno::NOENT);
                }
            }
            None
        }));
        let raced = state_usage(own.path()).unwrap();
        assert!(
            reads.load(std::sync::atomic::Ordering::SeqCst) >= 2,
            "listing did not continue after the first entry"
        );
        space_check(own.path(), 0, &RetentionConfig::default()).unwrap_or_else(|error| {
            panic!("directory removed during listing reported as NoSpace: {error}")
        });
        assert_eq!(accepted(raced), SpaceVerdict::Ok);
    }

    #[test]
    fn directory_list_eacces_propagates() {
        let own = ::test_support::private_tempdir().unwrap();
        let sub = own.path().join("sub");
        ::test_support::private_dir(&sub).unwrap();
        let _clear = install_dir_list_hook(Box::new(|name| {
            (name == b"sub").then_some(rustix::io::Errno::ACCESS)
        }));
        let error = state_usage(own.path()).unwrap_err();
        assert_eq!(
            error.raw_os_error(),
            Some(rustix::io::Errno::ACCESS.raw_os_error())
        );
    }

    #[test]
    fn readdir_eio_propagates() {
        let own = ::test_support::private_tempdir().unwrap();
        let sub = own.path().join("sub");
        ::test_support::private_dir(&sub).unwrap();
        let _clear = install_readdir_hook(Box::new(|name| {
            (name == b"sub").then_some(rustix::io::Errno::IO)
        }));
        let error = state_usage(own.path()).unwrap_err();
        assert_eq!(
            error.raw_os_error(),
            Some(rustix::io::Errno::IO.raw_os_error())
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

    fn policy(pct: f64, cap: u64) -> RetentionConfig {
        RetentionConfig {
            min_free_pct: pct,
            max_state_size: tidemark_core::units::ByteSize(cap),
            ..RetentionConfig::default()
        }
    }
    fn check(
        totals: Option<FsTotals>,
        used: u64,
        estimate: u64,
        pct: f64,
        cap: u64,
    ) -> SpaceVerdict {
        space_check_with(
            move || totals.ok_or_else(|| io::Error::other("stat failed")),
            used,
            estimate,
            &policy(pct, cap),
        )
        .unwrap()
    }
    fn assert_refusal(verdict: SpaceVerdict, kind: SpaceRefusal, reason: &str) {
        match verdict {
            SpaceVerdict::Refuse {
                kind: Some(got),
                reason: text,
            } => {
                assert_eq!(got, kind, "{text}");
                assert_eq!(text, reason);
            }
            SpaceVerdict::Refuse {
                kind: None,
                reason: text,
            } => {
                panic!("expected {kind:?} ({reason}), got an unclassified refusal: {text}")
            }
            SpaceVerdict::Ok => panic!("expected {kind:?} ({reason})"),
        }
    }

    #[test]
    fn every_space_guard_refusal_site_names_its_kind() {
        let huge = FsTotals {
            size: 100 << 30,
            avail: 80 << 30,
        };
        assert_refusal(
            check(Some(huge), 0, 0, f64::NAN, u64::MAX),
            SpaceRefusal::InvalidPolicy,
            "invalid minimum free percentage",
        );
        assert_refusal(
            check(Some(huge), 0, 0, f64::INFINITY, u64::MAX),
            SpaceRefusal::InvalidPolicy,
            "invalid minimum free percentage",
        );
        assert_refusal(
            check(Some(huge), 0, 0, -0.1, u64::MAX),
            SpaceRefusal::InvalidPolicy,
            "invalid minimum free percentage",
        );
        assert_refusal(
            check(Some(huge), 0, 0, 100.1, u64::MAX),
            SpaceRefusal::InvalidPolicy,
            "invalid minimum free percentage",
        );
        let unreadable = space_check_with(
            || Err(io::Error::other("stat failed")),
            0,
            0,
            &policy(5.0, u64::MAX),
        )
        .expect_err("unreadable totals are an I/O error");
        assert_eq!(unreadable.to_string(), "stat failed");
        assert!(matches!(
            crate::StoreError::from(unreadable),
            crate::StoreError::Io(_)
        ));
        assert_refusal(
            check(Some(huge), 80, 30, 5.0, 100),
            SpaceRefusal::MaxStateSize,
            "maximum state size would be exceeded",
        );
        assert_refusal(
            check(Some(huge), u64::MAX, 1, 5.0, u64::MAX),
            SpaceRefusal::MaxStateSize,
            "maximum state size would be exceeded",
        );
        assert_refusal(
            check(
                Some(FsTotals {
                    size: 4 << 30,
                    avail: 1 << 30,
                }),
                0,
                0,
                0.0,
                u64::MAX,
            ),
            SpaceRefusal::Floor {
                bytes: MANDATORY_FREE_BYTES,
                minimum_applied: true,
            },
            "state filesystem free-space floor would be crossed",
        );
        assert_refusal(
            check(
                Some(FsTotals {
                    size: 100 << 30,
                    avail: 10 << 30,
                }),
                0,
                0,
                50.0,
                u64::MAX,
            ),
            SpaceRefusal::Floor {
                bytes: 50 << 30,
                minimum_applied: false,
            },
            "state filesystem free-space floor would be crossed",
        );
        assert!(matches!(
            check(Some(huge), 0, 0, 5.0, u64::MAX),
            SpaceVerdict::Ok
        ));
        // Both limits at once: the cap is checked first and wins.
        let tight = FsTotals {
            size: 100 << 30,
            avail: 1 << 30,
        };
        assert_refusal(
            check(Some(tight), 0, 128 << 10, 50.0, 64 << 10),
            SpaceRefusal::MaxStateSize,
            "maximum state size would be exceeded",
        );
        assert_refusal(
            check(Some(tight), 0, 128 << 10, 50.0, u64::MAX),
            SpaceRefusal::Floor {
                bytes: 50 << 30,
                minimum_applied: false,
            },
            "state filesystem free-space floor would be crossed",
        );

        // Numerator overflow: size * mantissa does not fit in u128.
        assert_eq!(scaled_numerator(u128::MAX, 2), Err(SpaceRefusal::Overflow));
        // Denominator overflow: 2^shift * 100 does not fit. Valid percentages
        // never reach this multiply (shift >= 121 returns early).
        assert_eq!(percent_denominator(122), Err(SpaceRefusal::Overflow));
        // Width overflow: max(percentage, 2 GiB) does not fit in u64.
        assert_eq!(
            apply_mandatory(u128::from(u64::MAX) + 1),
            Err(SpaceRefusal::Overflow)
        );
        assert_eq!(OVERFLOW_REASON, "free-space floor overflow");
        assert_eq!(
            percentage_bytes(u64::MAX, 100.0).map(|p| apply_mandatory(p).map(|_| ())),
            Ok(Ok(()))
        );
    }
}

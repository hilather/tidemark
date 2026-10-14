//! Owned by: WLEASE
//! State-wide exclusion between live writers and database replacement.
use rustix::fs::{flock, FlockOperation};
use std::{
    ffi::OsStr,
    fs::File,
    io,
    path::Path,
    time::{Duration, Instant},
};

/// A bounded lease refusal, retained as the source of an I/O error.
#[derive(Debug, thiserror::Error)]
pub enum LeaseBusy {
    /// A recovery operation currently excludes ordinary writers.
    #[error("state directory is being recovered by another process")]
    Recovery,
    /// A live writer or another recovery operation excludes replacement.
    #[error("state directory in use by another tidemark process")]
    Writer,
}
fn acquire(state: &Path, wait: Duration, exclusive: bool) -> io::Result<File> {
    let directory = crate::state_dir::StateDir::open(state, true)?;
    let file = match directory.open_leaf(OsStr::new("writer.lock"), true, true) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            directory.open_leaf(OsStr::new("writer.lock"), true, false)?
        }
        result => result?,
    };
    let deadline = Instant::now() + wait;
    loop {
        match flock(
            &file,
            if exclusive {
                FlockOperation::NonBlockingLockExclusive
            } else {
                FlockOperation::NonBlockingLockShared
            },
        ) {
            Ok(()) => {
                verify_lock_identity(&directory, state, &file, OsStr::new("writer.lock"))?;
                return Ok(file);
            }
            Err(rustix::io::Errno::WOULDBLOCK) => {
                #[cfg(test)]
                AFTER_CONTENTION.with(|hook| {
                    if let Some(hook) = hook.borrow_mut().take() {
                        hook();
                    }
                });
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        if exclusive {
                            LeaseBusy::Writer
                        } else {
                            LeaseBusy::Recovery
                        },
                    ));
                }
                std::thread::sleep(
                    Duration::from_millis(25)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(rustix::io::Errno::INTR) if Instant::now() < deadline => continue,
            Err(error) => return Err(error.into()),
        }
    }
}
/// Refuse ownership of an unlinked lock or a detached/replaced state directory.
/// The named lock is checked relative to the directory retained before flock.
pub fn verify_lock_identity(
    directory: &crate::state_dir::StateDir,
    state: &Path,
    file: &File,
    name: &OsStr,
) -> io::Result<()> {
    let valid = (|| -> io::Result<bool> {
        use rustix::fs::{fstat, statat, AtFlags};
        let held = fstat(file)?;
        let named = statat(directory.fd(), name, AtFlags::SYMLINK_NOFOLLOW)?;
        let opened = fstat(directory.fd())?;
        let current = crate::state_dir::StateDir::open(state, false)?;
        let current = fstat(current.fd())?;
        Ok(held.st_nlink == 1
            && named.st_nlink == 1
            && held.st_dev == named.st_dev
            && held.st_ino == named.st_ino
            && opened.st_nlink != 0
            && opened.st_dev == current.st_dev
            && opened.st_ino == current.st_ino)
    })();
    if !matches!(valid, Ok(true)) {
        return Err(io::Error::new(io::ErrorKind::WouldBlock, LeaseBusy::Writer));
    }
    Ok(())
}
/// Shared lifetime lease for a write-capable connection.
/// flock is per open file description: local connections must be closed before
/// attempting an exclusive lease, just like connections in another process.
pub struct WriterLease {
    _file: File,
}
impl WriterLease {
    /// Acquire writer ownership with at most two seconds of contention retry.
    pub fn acquire(state: &Path) -> io::Result<Self> {
        Ok(Self {
            _file: acquire(state, Duration::from_secs(2), false)?,
        })
    }
}
/// Exclusive authority to replace shared databases and their sidecars.
/// Acquire this before the scan lock. All local writer connections must be closed.
pub struct RecoveryLease {
    file: File,
}
impl RecoveryLease {
    /// Acquire exclusive ownership using a caller-supplied bounded wait.
    pub fn acquire(state: &Path, wait: Duration) -> io::Result<Self> {
        Ok(Self {
            file: acquire(state, wait, true)?,
        })
    }
    /// Verify that this guard belongs to the requested state directory.
    pub fn verify(&self, state: &Path) -> io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let directory = crate::state_dir::StateDir::open(state, false)?;
        let named = directory
            .open_leaf(OsStr::new("writer.lock"), false, false)?
            .metadata()?;
        let held = self.file.metadata()?;
        if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recovery lease belongs to another state directory",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static AFTER_CONTENTION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiter_on_deleted_lock_never_gets_ownership() {
        for replace in [false, true] {
            let temp = test_support::private_tempdir().unwrap();
            let state = temp.path().join("state");
            let exclusive = RecoveryLease::acquire(&state, Duration::ZERO).unwrap();
            let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
            let (go_tx, go_rx) = std::sync::mpsc::sync_channel(1);
            let waiter_state = state.clone();
            let waiter = std::thread::spawn(move || {
                AFTER_CONTENTION.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        ready_tx.send(()).unwrap();
                        go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    }))
                });
                WriterLease::acquire(&waiter_state).err().unwrap().kind()
            });
            ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            std::fs::remove_file(state.join("writer.lock")).unwrap();
            if replace {
                std::fs::rename(&state, temp.path().join("moved")).unwrap();
                std::fs::create_dir(&state).unwrap();
            } else {
                std::fs::remove_dir(&state).unwrap();
            }
            drop(exclusive);
            go_tx.send(()).unwrap();
            assert_eq!(waiter.join().unwrap(), io::ErrorKind::WouldBlock);
        }
    }
}

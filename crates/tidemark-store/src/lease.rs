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
            Ok(()) => return Ok(file),
            Err(rustix::io::Errno::WOULDBLOCK) => {
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

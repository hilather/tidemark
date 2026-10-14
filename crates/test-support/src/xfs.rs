//! Owned by: W0-C
//! VM-only loop XFS lifecycle with rollback and conservative cleanup.
#[cfg(feature = "privileged")]
use crate::fixture::OwnedStorage;
#[cfg(feature = "privileged")]
use std::{ffi::OsStr, fs::File, path::PathBuf};
use std::{
    ffi::OsString,
    io,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
#[cfg(feature = "privileged")]
use tempfile::TempDir;

const CLEANUP_DEADLINE: Duration = Duration::from_secs(3);

fn wait_cleanup_child(mut child: Child, timeout: Duration) -> io::Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                // Own the subprocess through termination and reaping, including
                // polling errors. No detached worker may retain the fixture.
                let killed = child.kill();
                let reaped = child.wait();
                killed?;
                reaped?;
                return Err(match result {
                    Err(error) => error,
                    _ => io::Error::new(
                        io::ErrorKind::TimedOut,
                        "fixture cleanup command exceeded deadline",
                    ),
                });
            }
        }
    }
}

fn run_cleanup(program: &str, args: &[OsString], timeout: Duration) -> io::Result<()> {
    // Cleanup needs only status. Null streams avoid blocking on inherited pipes.
    let child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let status = wait_cleanup_child(child, timeout)?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{program} cleanup failed ({status})"
        )))
    }
}

/// Require real root privileges before any loop/mount operation.
#[cfg(feature = "privileged")]
pub(crate) fn require_root() {
    assert!(
        rustix::process::geteuid().is_root(),
        "privileged fixture requires root in a disposable VM; use cargo xtask vm-test"
    );
}
#[cfg(feature = "privileged")]
pub(crate) fn run(program: &str, args: &[OsString]) -> io::Result<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "{program} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
#[cfg(feature = "privileged")]
pub(crate) fn words(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}
/// Owned loop filesystem. Drop unmounts and detaches before deleting storage.
#[cfg(feature = "privileged")]
#[derive(Debug)]
pub struct LoopXfs {
    /// Backing image, inside the owned temporary directory.
    pub image: PathBuf,
    /// XFS mount path, inside the owned temporary directory.
    pub mountpoint: PathBuf,
    dir: Option<TempDir>,
    storage: Option<OwnedStorage>,
    loopdev: Option<OsString>,
    mounted: bool,
}
#[cfg(feature = "privileged")]
impl LoopXfs {
    /// Create and mount XFS; panics clearly when not root or commands fail.
    /// `size` is bytes; use at least 512 MiB with current xfsprogs.
    pub fn new(size: u64, mkfs_opts: &[&str], mount_opts: &str) -> Self {
        Self::try_new(size, mkfs_opts, mount_opts)
            .unwrap_or_else(|e| panic!("LoopXfs setup failed: {e}"))
    }
    /// Fallible constructor; partially constructed resources are rolled back.
    pub fn try_new(size: u64, mkfs_opts: &[&str], mount_opts: &str) -> io::Result<Self> {
        require_root();
        let mut dir = crate::private_tempdir()?;
        let storage = OwnedStorage::new(&mut dir)?;
        let image = dir.path().join("xfs.img");
        let mountpoint = dir.path().join("mount");
        File::create(&image)?.set_len(size)?;
        std::fs::create_dir(&mountpoint)?;
        let mut fs = Self {
            image,
            mountpoint,
            dir: Some(dir),
            storage: Some(storage),
            loopdev: None,
            mounted: false,
        };
        let mut args = words(&["--find", "--show", "--"]);
        args.push(fs.image.clone().into_os_string());
        let device = run("losetup", &args)?;
        if !device.starts_with("/dev/loop") {
            return Err(io::Error::other("losetup returned unexpected device"));
        }
        fs.loopdev = Some(device.into());
        let device = fs
            .loopdev
            .as_ref()
            .ok_or_else(|| io::Error::other("loop device absent"))?;
        let mut args = words(&["-f"]);
        args.extend(mkfs_opts.iter().map(OsString::from));
        args.push(device.clone());
        run("mkfs.xfs", &args)?;
        let mut args = words(&["-t", "xfs"]);
        if !mount_opts.is_empty() {
            args.extend([OsString::from("-o"), OsString::from(mount_opts)]);
        }
        args.extend([device.clone(), fs.mountpoint.clone().into_os_string()]);
        run("mount", &args)?;
        fs.mounted = true;
        Ok(fs)
    }
    /// Explicit cleanup, allowing a test to assert successful resource removal.
    pub fn cleanup(&mut self) -> io::Result<()> {
        if let Some(storage) = &self.storage {
            storage.verify_owner()?;
            if let Some(dir) = &self.dir {
                storage.verify_path(dir.path())?;
            }
        }
        if self.mounted {
            let expected = self.dir.as_ref().map(|d| d.path().join("mount"));
            if expected.as_ref() != Some(&self.mountpoint) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing modified mountpoint outside owned loop fixture",
                ));
            }
            run_cleanup(
                "umount",
                &[self.mountpoint.clone().into_os_string()],
                CLEANUP_DEADLINE,
            )?;
            self.mounted = false;
        }
        if let Some(device) = &self.loopdev {
            run_cleanup(
                "losetup",
                &[OsStr::new("--detach").into(), device.clone()],
                CLEANUP_DEADLINE,
            )?;
            self.loopdev = None;
        }
        if let Some(storage) = &self.storage {
            storage.cleanup(&[])?;
        }
        self.storage = None;
        self.dir = None;
        Ok(())
    }
}
#[cfg(feature = "privileged")]
impl Drop for LoopXfs {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            eprintln!("LoopXfs cleanup failed; preserving backing storage: {e}");
            if let Some(dir) = self.dir.take() {
                let _ = dir.keep();
            }
        }
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    #[test]
    fn stalled_cleanup_is_killed_and_reaped_before_error() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let started = Instant::now();
        let error = wait_cleanup_child(child, Duration::from_millis(30)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        assert!(run_cleanup("true", &[], CLEANUP_DEADLINE).is_ok());
    }
}

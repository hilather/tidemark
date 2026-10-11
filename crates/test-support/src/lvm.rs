//! Owned by: W0-C
//! VM-only loop PV/VG/LV fixtures restricted to uniquely owned tmtest_ names.
use crate::xfs::{require_root, run, words};
use std::{ffi::OsString, fs::File, io, path::PathBuf};
use tempfile::TempDir;
/// Optional linear and thin layouts, all sizes in bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct LvLayout {
    /// Linear LV size.
    pub linear: Option<u64>,
    /// Thin-pool size and thin-LV virtual size.
    pub thin_pool: Option<(u64, u64)>,
}
/// Mounted test logical volume.
#[derive(Debug)]
pub struct TestLv {
    /// LV device path.
    pub device: PathBuf,
    /// Mountpoint in the owned temporary directory.
    pub mountpoint: PathBuf,
}
/// Owned test VG; its immutable name always begins with `tmtest_`.
#[derive(Debug)]
pub struct LoopVg {
    /// Backing PV image.
    pub image: PathBuf,
    /// Successfully mounted LVs.
    pub mounts: Vec<TestLv>,
    name: String,
    dir: Option<TempDir>,
    loopdev: Option<OsString>,
    pv_created: bool,
    vg_created: bool,
    lvs_created: bool,
}
impl LoopVg {
    /// Create a unique VG, format and mount requested LVs; requires a disposable root VM.
    pub fn new(name_suffix: &str, pv_size: u64, layout: LvLayout) -> Self {
        Self::try_new(name_suffix, pv_size, layout)
            .unwrap_or_else(|e| panic!("LoopVg setup failed: {e}"))
    }
    /// Immutable VG name for reporting-command comparisons.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Fallible setup with rollback of successfully created resources.
    pub fn try_new(suffix: &str, pv_size: u64, layout: LvLayout) -> io::Result<Self> {
        require_root();
        if suffix.is_empty()
            || suffix.len() > 64
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "VG suffix must contain only letters, digits or underscores",
            ));
        }
        let dir = tempfile::tempdir()?;
        let unique = dir
            .path()
            .file_name()
            .ok_or_else(|| io::Error::other("missing temporary name"))?
            .to_string_lossy()
            .replace('.', "_");
        let name = format!("tmtest_{suffix}_{}_{}", std::process::id(), unique);
        let image = dir.path().join("pv.img");
        File::create(&image)?.set_len(pv_size)?;
        let mut vg = Self {
            image,
            mounts: vec![],
            name,
            dir: Some(dir),
            loopdev: None,
            pv_created: false,
            vg_created: false,
            lvs_created: false,
        };
        vg.guard()?;
        // Never claim or remove a pre-existing VG, even in the improbable name collision.
        let existing = run("vgs", &words(&["--noheadings", "-o", "vg_name"]))?;
        if existing.lines().any(|n| n.trim() == vg.name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test VG name exists",
            ));
        }
        let mut args = words(&["--find", "--show", "--"]);
        args.push(vg.image.clone().into_os_string());
        let dev = run("losetup", &args)?;
        if !dev.starts_with("/dev/loop") {
            return Err(io::Error::other("unexpected loop device"));
        }
        vg.loopdev = Some(dev.into());
        let dev = vg
            .loopdev
            .as_ref()
            .ok_or_else(|| io::Error::other("missing loop device"))?
            .clone();
        run("pvcreate", &["--yes".into(), dev.clone()])?;
        vg.pv_created = true;
        run("vgcreate", &[vg.name.clone().into(), dev])?;
        vg.vg_created = true;
        if let Some(size) = layout.linear {
            run(
                "lvcreate",
                &words(&["--yes", "-L", &format!("{size}B"), "-n", "linear", &vg.name]),
            )?;
            vg.lvs_created = true;
            vg.mount_lv("linear")?;
        }
        if let Some((pool, virtual_size)) = layout.thin_pool {
            run(
                "lvcreate",
                &words(&[
                    "--yes",
                    "-L",
                    &format!("{pool}B"),
                    "--type",
                    "thin-pool",
                    "-n",
                    "pool",
                    &vg.name,
                ]),
            )?;
            vg.lvs_created = true;
            run(
                "lvcreate",
                &words(&[
                    "--yes",
                    "-V",
                    &format!("{virtual_size}B"),
                    "-T",
                    &format!("{}/pool", vg.name),
                    "-n",
                    "thin",
                ]),
            )?;
            vg.mount_lv("thin")?;
        }
        Ok(vg)
    }
    fn guard(&self) -> io::Result<()> {
        if !self.name.starts_with("tmtest_") {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing non-tmtest_ VG",
            ))
        } else {
            Ok(())
        }
    }
    fn mount_lv(&mut self, lv: &str) -> io::Result<()> {
        self.guard()?;
        let device = PathBuf::from(format!("/dev/{}/{lv}", self.name));
        let dir = self
            .dir
            .as_ref()
            .ok_or_else(|| io::Error::other("missing owner directory"))?;
        let mountpoint = dir.path().join(lv);
        std::fs::create_dir(&mountpoint)?;
        run("mkfs.xfs", &["-f".into(), device.clone().into_os_string()])?;
        run(
            "mount",
            &[
                device.clone().into_os_string(),
                mountpoint.clone().into_os_string(),
            ],
        )?;
        self.mounts.push(TestLv { device, mountpoint });
        Ok(())
    }
    /// Remove owned resources in dependency order. Stops on cleanup failure.
    pub fn cleanup(&mut self) -> io::Result<()> {
        self.guard()?;
        while let Some(lv) = self.mounts.last() {
            let owner = self
                .dir
                .as_ref()
                .ok_or_else(|| io::Error::other("missing mount owner"))?;
            if !["linear", "thin"]
                .iter()
                .any(|name| lv.mountpoint == owner.path().join(name))
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing modified mountpoint outside owned VG fixture",
                ));
            }
            run("umount", &[lv.mountpoint.clone().into_os_string()])?;
            self.mounts.pop();
        }
        if self.lvs_created {
            run("lvremove", &words(&["--yes", "--force", &self.name]))?;
            self.lvs_created = false;
        }
        if self.vg_created {
            run("vgremove", &words(&["--yes", "--force", &self.name]))?;
            self.vg_created = false;
        }
        if let Some(dev) = &self.loopdev {
            if self.pv_created {
                run("pvremove", &["--yes".into(), dev.clone()])?;
                self.pv_created = false;
            }
            run("losetup", &["--detach".into(), dev.clone()])?;
            self.loopdev = None;
        }
        if let Some(dir) = self.dir.take() {
            dir.close()?;
        }
        Ok(())
    }
}
impl Drop for LoopVg {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            eprintln!("LoopVg cleanup failed; preserving backing storage: {e}");
            if let Some(dir) = self.dir.take() {
                let _ = dir.keep();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_non_test_vg_without_commands() {
        let mut vg = LoopVg {
            image: PathBuf::new(),
            mounts: vec![],
            name: "production".into(),
            dir: None,
            loopdev: None,
            pv_created: false,
            vg_created: false,
            lvs_created: false,
        };
        assert_eq!(
            vg.cleanup().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        vg.name = "tmtest_unit".into();
    }
}

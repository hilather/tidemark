//! Owned by: W7-C3-fuzz
//! Exercise record readers with inert service and API boundaries.
use std::{fs, io, os::unix::fs::PermissionsExt, time::Duration};
use tidemark_cli::deploy::{ApiPing, Systemctl};
struct Inert;
impl Systemctl for Inert {
    fn is_enabled(&self, _: &str) -> io::Result<bool> {
        Ok(false)
    }
    fn is_active(&self, _: &str) -> io::Result<bool> {
        Ok(false)
    }
    fn enable(&self, _: &str) -> io::Result<()> {
        panic!("verify must never enable")
    }
    fn disable_now(&self, _: &str) -> io::Result<()> {
        panic!("verify must never disable")
    }
    fn restart(&self, _: &str) -> io::Result<()> {
        panic!("verify must never restart")
    }
}
impl ApiPing for Inert {
    fn ping(&self) -> io::Result<()> {
        Ok(())
    }
}
pub fn exercise(bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let deploy = dir.path().join("deploy");
    fs::create_dir(&deploy).unwrap();
    fs::set_permissions(&deploy, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["staged.json", "firstboot.json"] {
        fs::write(deploy.join(name), bytes).unwrap();
    }
    let result = tidemark_cli::deploy::verify(dir.path(), Duration::ZERO, &Inert, &Inert);
    if bytes.len() > 1024 * 1024 {
        assert!(result.is_err());
    }
    if bytes == b"{}" {
        let result = result.unwrap();
        assert!(result.staged.is_some());
        assert!(result.firstboot.is_some());
    }
}

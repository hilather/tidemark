//! Owned by: W5-C
//! VM-only PostgreSQL fixture; run the privileged-feature tests as an unprivileged user.
use std::{path::PathBuf, process::Command};

pub struct PgTmp {
    pub dir: tempfile::TempDir,
    bin: PathBuf,
}
impl PgTmp {
    pub fn start() -> Self {
        let dir = tempfile::tempdir().expect("temporary PostgreSQL cluster");
        let bin = std::env::var_os("PG_BINDIR")
            .map(PathBuf::from)
            .unwrap_or_default();
        let data = dir.path().join("data");
        let status = Command::new(bin.join("initdb"))
            .args(["-A", "trust", "-U", "postgres", "--no-locale"])
            .arg("-D")
            .arg(&data)
            .status()
            .expect("VM needs initdb in PATH or PG_BINDIR");
        assert!(status.success(), "run fixture as an unprivileged VM user");
        let fixture = Self { dir, bin };
        let dir = &fixture.dir;
        let bin = &fixture.bin;
        let socket = dir.path().to_string_lossy().replace('\'', "'\\''");
        let options = format!("-F -k '{socket}' -c listen_addresses='' -p 55432");
        let status = Command::new(bin.join("pg_ctl"))
            .arg("-D")
            .arg(&data)
            .arg("-l")
            .arg(dir.path().join("server.log"))
            .args(["-w", "-o", &options, "start"])
            .status()
            .expect("pg_ctl start");
        assert!(status.success());
        fixture
    }
    pub fn dsn(&self, user: &str) -> String {
        let socket = self
            .dir
            .path()
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'");
        format!("host='{socket}' port=55432 user={user} dbname=postgres")
    }
    pub fn admin(&self) -> postgres::Client {
        postgres::Client::connect(&self.dsn("postgres"), postgres::NoTls)
            .expect("fixture admin connection")
    }
}
impl Drop for PgTmp {
    fn drop(&mut self) {
        let _ = Command::new(self.bin.join("pg_ctl"))
            .arg("-D")
            .arg(self.dir.path().join("data"))
            .args(["-w", "-m", "immediate", "stop"])
            .status();
    }
}

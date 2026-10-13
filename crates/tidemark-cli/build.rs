//! Owned by: WPKG-build-id
//! Best-effort, dependency-free build identity.

#![forbid(unsafe_code)]

use std::{env, path::PathBuf, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn identity(
    commit: Option<String>,
    date: Option<String>,
    git_commit: Option<String>,
    git_date: Option<String>,
    dirty: bool,
) -> (String, String) {
    let commit = commit.unwrap_or_else(|| {
        git_commit
            .map(|c| if dirty { format!("{c}-dirty") } else { c })
            .unwrap_or_else(|| "unknown".into())
    });
    (
        commit,
        date.or(git_date).unwrap_or_else(|| "unknown".into()),
    )
}

fn main() {
    for key in ["TIDEMARK_BUILD_COMMIT", "TIDEMARK_BUILD_DATE"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default()).join("../..");
    let head = root.join(".git/HEAD");
    if head.is_file() {
        println!("cargo:rerun-if-changed={}", head.display());
    }
    // Resolve worktree git directories as well as ordinary checkouts.
    for args in [
        &["rev-parse", "--git-path", "HEAD"][..],
        &["rev-parse", "--git-path", "index"][..],
    ] {
        if let Some(path) = git(args) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    // Track source edits so an incremental local build refreshes the dirty suffix.
    if let Some(paths) = git(&["ls-files", "--full-name", "../../"]) {
        for path in paths.lines() {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    let (commit, date) = identity(
        env::var("TIDEMARK_BUILD_COMMIT").ok(),
        env::var("TIDEMARK_BUILD_DATE").ok(),
        git(&["rev-parse", "--short=12", "HEAD"]),
        git(&["show", "-s", "--format=%cs", "HEAD"]),
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty()),
    );
    for (key, value) in [
        ("TIDEMARK_BUILD_COMMIT", commit.clone()),
        ("TIDEMARK_BUILD_DATE", date.clone()),
    ] {
        println!("cargo:rustc-env={key}={}", value.replace(['\n', '\r'], " "));
    }
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".into());
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=TIDEMARK_BUILD_TARGET={target}");
    println!("cargo:rustc-env=TIDEMARK_BUILD_PROFILE={profile}");
}

#[cfg(test)]
mod tests {
    use super::identity;
    #[test]
    fn identity_sources() {
        let fake = Some("012345abcdef".into());
        let date = Some("2026-10-13".into());
        assert_eq!(
            identity(fake.clone(), date.clone(), None, None, true),
            ("012345abcdef".into(), "2026-10-13".into())
        );
        assert_eq!(
            identity(None, None, fake.clone(), date.clone(), false),
            ("012345abcdef".into(), "2026-10-13".into())
        );
        assert_eq!(
            identity(None, None, fake, date, true),
            ("012345abcdef-dirty".into(), "2026-10-13".into())
        );
        assert_eq!(
            identity(None, None, None, None, false),
            ("unknown".into(), "unknown".into())
        );
    }
}

//! Owned by: W3-C
//! Command surface and shared CLI support.

/// Command arguments.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Operation.
    #[command(subcommand)]
    pub command: Command,
}
/// Available operations.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// ncdu JSON export.
    Ncdu {
        /// Directory path to query (canonicalised before lookup).
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
        /// Write ncdu JSON to this file.
        #[arg(short = 'o', value_name = "FILE")]
        output: Option<std::path::PathBuf>,
        /// Snapshot ID to query.
        #[arg(long, value_name = "ID")]
        snapshot: Option<String>,
        /// Replace invalid UTF-8 in exported names.
        #[arg(long)]
        utf8_lossy: bool,
        /// Maximum directory depth to export.
        #[arg(long, value_name = "N")]
        max_depth: Option<usize>,
    },
}
/// Execute the operation using an immutable selected snapshot.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use std::io::Write;
    let Command::Ncdu {
        path,
        output,
        snapshot,
        utf8_lossy,
        max_depth,
    } = &args.command;
    let max_depth = max_depth
        .map(u32::try_from)
        .transpose()
        .map_err(|_| crate::CliError::Configuration("max-depth exceeds u32".into()))?;
    let (path, snap) = super::snapshots::select(context, path.as_deref(), snapshot.as_deref())?;
    let opts = tidemark_store::NcduOptions {
        utf8_lossy: *utf8_lossy,
        max_depth,
    };
    if let Some(output) = output {
        let (mut file, target) = create_output(output, &context.config.general.state_dir)?;
        {
            let mut out = std::io::BufWriter::new(file.as_file_mut());
            tidemark_store::export_ncdu(&snap, &path, &mut out, &opts)?;
            out.flush()?;
        }
        publish_output(&file, &target)?;
    } else {
        let mut out = std::io::BufWriter::new(std::io::stdout().lock());
        tidemark_store::export_ncdu(&snap, &path, &mut out, &opts)?;
        out.flush()?;
    }
    Ok(())
}
fn create_output(
    output: &std::path::Path,
    state: &std::path::Path,
) -> anyhow::Result<(tempfile::NamedTempFile, std::path::PathBuf)> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let parent = std::fs::canonicalize(parent)?;
    let name = output
        .file_name()
        .ok_or_else(|| crate::CliError::Configuration("output requires a file name".into()))?;
    let target = parent.join(name);
    let state = std::fs::canonicalize(state)?;
    if target.starts_with(&state) {
        return Err(
            crate::CliError::Precondition("refusing export inside state_dir".into()).into(),
        );
    }
    match std::fs::symlink_metadata(&target) {
        Ok(_) => return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // NamedTempFile creates a private 0600 file in this same directory. A crash
    // can leave its conspicuous temporary name, never an incomplete target.
    let file = tempfile::Builder::new()
        .prefix(".tidemark-export-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    Ok((file, target))
}

fn publish_output(file: &tempfile::NamedTempFile, target: &std::path::Path) -> anyhow::Result<()> {
    file.as_file().sync_all()?;
    // NOREPLACE preserves refusal to overwrite if a destination appears during
    // export. The temporary file is removed on error/drop.
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        file.path(),
        rustix::fs::CWD,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn staging_is_private_and_publication_never_replaces_a_racing_target() {
        use super::{create_output, publish_output};
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let temp = ::test_support::private_tempdir().unwrap();
        let state = temp.path().join("state");
        ::test_support::private_dir(&state).unwrap();
        let target = temp.path().join("export.json");
        let (mut file, target) = create_output(&target, &state).unwrap();
        let staging = file.path().to_owned();
        file.write_all(b"partial").unwrap();
        assert!(!target.exists());
        assert_eq!(
            file.as_file().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::write(&target, b"racing destination").unwrap();
        assert!(publish_output(&file, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"racing destination");
        drop(file);
        assert!(!staging.exists());
    }
}

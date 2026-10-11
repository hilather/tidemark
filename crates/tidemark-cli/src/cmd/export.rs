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
        let mut file = create_output(output, &context.config.general.state_dir)?;
        let mut out = std::io::BufWriter::new(&mut file);
        tidemark_store::export_ncdu(&snap, &path, &mut out, &opts)?;
        out.flush()?;
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
) -> anyhow::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
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
    // Exclusive creation refuses symlinks, hard links and all existing files,
    // including snapshots located outside the configured state directory.
    Ok(std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)?)
}

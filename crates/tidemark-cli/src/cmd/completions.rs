//! Owned by: W0-B2
//! Shell completion generation.

use clap::CommandFactory;
/// Completion arguments.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Target shell.
    #[arg(value_enum, value_name = "SHELL")]
    pub shell: clap_complete::Shell,
}
/// Generate the shell's native completion script.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let mut script = Vec::new();
    clap_complete::generate(
        args.shell,
        &mut crate::cli::Cli::command(),
        "tidemark",
        &mut script,
    );
    let mut output = std::io::stdout().lock();
    if context.json {
        crate::render::json(
            "tidemark.completions.v1",
            String::from_utf8(script)?,
            &mut output,
        )?;
    } else {
        use std::io::Write;
        output.write_all(&script)?;
    }
    Ok(())
}

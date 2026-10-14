//! Owned by: W6-A1
//! Thin daemon executable entry point.

#![forbid(unsafe_code)]
fn main() -> std::process::ExitCode {
    // Keep the library command contract while extending the executable's version.
    if std::env::args_os().any(|arg| arg == "--version") {
        use clap::CommandFactory;
        let command = tidemarkd::Args::command();
        let version = if tidemark_store::state_dir::strict_ancestry() {
            concat!(
                env!("CARGO_PKG_VERSION"),
                "\ncommit: ",
                env!("TIDEMARK_BUILD_COMMIT"),
                " (",
                env!("TIDEMARK_BUILD_DATE"),
                ")\ntarget: ",
                env!("TIDEMARK_BUILD_TARGET"),
                ", profile ",
                env!("TIDEMARK_BUILD_PROFILE"),
                "\nancestry checks: strict"
            )
        } else {
            concat!(
                env!("CARGO_PKG_VERSION"),
                "\ncommit: ",
                env!("TIDEMARK_BUILD_COMMIT"),
                " (",
                env!("TIDEMARK_BUILD_DATE"),
                ")\ntarget: ",
                env!("TIDEMARK_BUILD_TARGET"),
                ", profile ",
                env!("TIDEMARK_BUILD_PROFILE"),
                "\nancestry checks: development (relaxed)"
            )
        };
        command.long_version(version).get_matches();
    }
    tidemarkd::main_entry()
}

//! Owned by: W6-A
//! Thin daemon executable entry point.

#![forbid(unsafe_code)]
fn main() -> std::process::ExitCode {
    tidemarkd::main_entry()
}

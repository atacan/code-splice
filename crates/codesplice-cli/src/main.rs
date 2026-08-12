#![forbid(unsafe_code)]
//! CodeSplice executable entry point.

fn main() -> std::process::ExitCode {
    codesplice_cli::run()
}

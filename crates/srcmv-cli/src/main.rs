#![forbid(unsafe_code)]
//! CodeSplice executable entry point.

fn main() -> std::process::ExitCode {
    srcmv_cli::run()
}

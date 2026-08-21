#![forbid(unsafe_code)]
//! srcmv executable entry point.

fn main() -> std::process::ExitCode {
    srcmv_cli::run()
}

//! Deterministic fake language server used by CodeSplice integration tests.

use std::process::ExitCode;

fn main() -> ExitCode {
    match codesplice_test_support::fake_lsp::run_from_process_args(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("codesplice-fake-lsp: {error}");
            ExitCode::from(2)
        }
    }
}

//! Output policy for compiler/toolchain subprocesses spawned by `perry compile`.
//!
//! Successful Cargo, rustc, clang, Swift, and linker output describes Perry's
//! implementation rather than the user's TypeScript. Keep it out of the
//! default UI, while retaining the complete stream for `--verbose` and
//! replaying it when the tool fails and it becomes actionable.

use std::io::{self, Write};
use std::process::{Command, ExitStatus};

pub(crate) fn run_internal_tool(cmd: &mut Command, verbose: u8) -> io::Result<ExitStatus> {
    if verbose > 0 {
        return cmd.status();
    }

    let output = cmd.output()?;
    if !output.status.success() {
        // Preserve the child's stdout/stderr split and make the actual compiler
        // error visible before the higher-level Perry context is printed.
        let _ = io::stdout().write_all(&output.stdout);
        let _ = io::stdout().flush();
        let _ = io::stderr().write_all(&output.stderr);
        let _ = io::stderr().flush();
    }
    Ok(output.status)
}

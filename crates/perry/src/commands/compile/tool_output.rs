//! Output policy for compiler/toolchain subprocesses spawned by `perry compile`.
//!
//! Successful Cargo, rustc, clang, Swift, and linker output describes Perry's
//! implementation rather than the user's TypeScript. Keep it out of the
//! default UI, while retaining the complete stream for `--verbose` and
//! replaying it when the tool fails and it becomes actionable.

use std::io::{self, Read, Write};
use std::process::{Command, ExitStatus, Stdio};

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

/// Retain the first diagnostic for a build whose failure is recoverable. The
/// caller can explain the fallback without burying its cause in Cargo output.
pub(crate) fn run_internal_tool_with_diagnostic(
    cmd: &mut Command,
    verbose: u8,
) -> io::Result<(ExitStatus, Option<String>)> {
    cmd.env("CARGO_TERM_COLOR", "never");
    if verbose > 0 {
        // Preserve live verbose output while retaining stderr for the summary.
        let mut child = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stderr = child.stderr.take().expect("piped stderr");
        let mut captured = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            let read = stderr.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            captured.extend_from_slice(&buffer[..read]);
            let _ = io::stderr().write_all(&buffer[..read]);
        }
        return Ok((child.wait()?, first_error_line(&captured)));
    }
    let output = cmd.output()?;
    let diagnostic = first_error_line(&output.stderr).or_else(|| first_error_line(&output.stdout));
    if !output.status.success() {
        let _ = io::stdout().write_all(&output.stdout);
        let _ = io::stdout().flush();
        let _ = io::stderr().write_all(&output.stderr);
        let _ = io::stderr().flush();
    }
    Ok((output.status, diagnostic))
}

fn first_error_line(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("error:") || line.starts_with("error["))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_diagnostic_skips_progress_and_keeps_first_error() {
        assert_eq!(
            first_error_line(b"   Compiling perry-stdlib\nwarning: unused import\nerror[E0425]: cannot find function `nanbox_handle_value`\nerror: could not compile\n"),
            Some("error[E0425]: cannot find function `nanbox_handle_value`".to_string())
        );
        assert_eq!(
            first_error_line(b"error: failed to download dependency\n"),
            Some("error: failed to download dependency".to_string())
        );
        assert_eq!(first_error_line(b"   Finished release profile\n"), None);
    }

    #[cfg(unix)]
    #[test]
    fn failed_tool_retains_diagnostic_and_exit_status() {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "echo 'error[E0425]: planted build failure' >&2; exit 42",
        ]);
        let (status, diagnostic) = run_internal_tool_with_diagnostic(&mut cmd, 0).unwrap();
        assert_eq!(status.code(), Some(42));
        assert_eq!(
            diagnostic.as_deref(),
            Some("error[E0425]: planted build failure")
        );
    }
}

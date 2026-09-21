//! Regression coverage for #10895: `for await (const chunk of process.stdin)`
//! on a pipe stalled forever part-way through the input.
//!
//! The async iterator pauses its source after every delivered chunk and
//! resumes it on the next pull. On `process.stdin`, `pause()` latches
//! `STDIN_DETACHED` (the fd-0 reader thread exits when it sees it) and
//! `resume()` clears the latch and respawns the reader unless one is still
//! registered. A `resume()` that landed while the old reader was on its way
//! out found it still registered, spawned nothing, and the old reader then
//! left: no reader on fd 0, every liveness view still reporting an open,
//! flowing stdin, the process idle forever with input unread.
//!
//! It is a race, so this test is statistical by nature: many small writes
//! (each pause/resume cycle is one roll) over several rounds. On an unpatched
//! build a single 8 MiB round fed in 256-byte writes stalls roughly every
//! second time on macOS and rarely on Linux; the deterministic witness for
//! the interleaving itself is `reader_lifecycle_tests` in
//! `perry-runtime/src/os_process_streams.rs`.

#![cfg(unix)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-files/test_issue_10895_stdin_pipe_stall.ts"
));

const TOTAL_BYTES: usize = 8 * 1024 * 1024;
const WRITE_BYTES: usize = 256;
const ROUNDS: usize = 12;
const ROUND_DEADLINE: Duration = Duration::from_secs(60);

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path) -> PathBuf {
    let source = dir.join("stdin_pipe_stall.ts");
    let binary = dir.join("stdin_pipe_stall_bin");
    std::fs::write(&source, SOURCE).expect("write stdin fixture");
    let output = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile stdin fixture");
    assert!(
        output.status.success(),
        "fixture compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

/// One round: pipe `TOTAL_BYTES` in `WRITE_BYTES` writes, close stdin, and
/// require the child to report every byte before the deadline.
fn run_round(binary: &Path, round: usize) {
    let mut child = Command::new(binary)
        .env("PERRY_10895_DRIVE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn stdin fixture");
    let mut stdin = child.stdin.take().expect("fixture stdin");
    let writer = std::thread::spawn(move || {
        let piece = [1u8; WRITE_BYTES];
        let mut left = TOTAL_BYTES;
        while left > 0 {
            let n = left.min(WRITE_BYTES);
            // A stalled child stops draining the pipe; the kill below then
            // breaks this write with EPIPE. Either way the thread ends.
            if stdin.write_all(&piece[..n]).is_err() {
                return left;
            }
            left -= n;
        }
        drop(stdin);
        0
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll stdin fixture") {
            break Some(status);
        }
        if started.elapsed() > ROUND_DEADLINE {
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        let unwritten = writer.join().unwrap_or(TOTAL_BYTES);
        panic!(
            "round {round}: piped stdin stalled — the child was still alive after {:?} with \
             {unwritten} of {TOTAL_BYTES} bytes not even accepted by the pipe (#10895)",
            ROUND_DEADLINE
        );
    };
    assert_eq!(writer.join().expect("writer thread"), 0);
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("fixture stdout")
        .read_to_string(&mut stdout)
        .expect("read fixture stdout");
    assert!(
        status.success(),
        "round {round}: exit {status:?}: {stdout:?}"
    );
    assert_eq!(
        stdout.trim_end(),
        format!("RESULT:{TOTAL_BYTES}"),
        "round {round}: not every piped byte reached the iterator"
    );
}

#[test]
fn piped_stdin_async_iteration_reads_to_eof_every_time() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let binary = compile(dir.path());
    for round in 0..ROUNDS {
        run_round(&binary, round);
    }
}

/// Undriven, the fixture must leave stdin alone (the parity sweep runs it
/// with whatever stdin the caller has).
#[test]
fn undriven_fixture_does_not_wait_on_stdin() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let binary = compile(dir.path());
    let output = Command::new(&binary)
        .stdin(Stdio::piped())
        .output()
        .expect("run undriven fixture");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "RESULT:idle"
    );
}

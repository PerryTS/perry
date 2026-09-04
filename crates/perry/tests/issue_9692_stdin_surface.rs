//! Regression coverage for #9692's shared stdin surface.
//!
//! Node and Perry run the same fixture with both possible initialization
//! orders: the runtime object reader requested first through an aliased stream,
//! or readline requested first through the syntactic `process.stdin` path.
//! Every scenario is exercised with a pipe, a cooked PTY, and a raw PTY. The
//! PTY arms are load-bearing: two competing `StdinLock` owners can appear green
//! under a pipe depending on which reader happens to start first.

#![cfg(unix)]

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-files/test_issue_9692_stdin_surface.ts"
));

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path) -> (PathBuf, PathBuf) {
    let source = dir.join("stdin_surface.ts");
    let binary = dir.join("stdin_surface_bin");
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
    (source, binary)
}

fn command_for(program: &Path, is_node: bool) -> Command {
    if is_node {
        let mut command = Command::new("node");
        command.arg("--no-warnings").arg(program);
        command
    } else {
        Command::new(program)
    }
}

fn configure(command: &mut Command, scenario: &str, order: &str, raw: bool) {
    command
        .env("PERRY_9692_SCENARIO", scenario)
        .env("PERRY_9692_ORDER", order)
        .env("PERRY_9692_RAW", if raw { "1" } else { "0" });
}

fn result_line(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.trim_end_matches('\r')
            .strip_prefix("RESULT:")
            .map(str::to_owned)
    })
}

fn run_pipe(program: &Path, is_node: bool, scenario: &str, order: &str) -> (ExitStatus, String) {
    let mut command = command_for(program, is_node);
    configure(&mut command, scenario, order, false);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pipe fixture");
    let mut stdin = child.stdin.take().expect("pipe fixture stdin");
    stdin.write_all(b"ab\n").expect("write pipe input");
    drop(stdin);
    let output = child.wait_with_output().expect("wait for pipe fixture");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let result = result_line(&stdout).unwrap_or_else(|| panic!("missing RESULT line: {stdout:?}"));
    (output.status, result)
}

fn open_pty() -> (File, File) {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, 0, "openpty failed: {}", std::io::Error::last_os_error());
    assert!(master >= 0 && slave >= 0);
    // SAFETY: openpty returned two fresh owned descriptors.
    unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
}

struct PtyChild {
    child: Child,
    input: File,
    lines: Receiver<String>,
}

impl PtyChild {
    fn spawn(program: &Path, is_node: bool, scenario: &str, order: &str, raw: bool) -> Self {
        let (master, slave) = open_pty();
        let mut command = command_for(program, is_node);
        configure(&mut command, scenario, order, raw);
        command
            .stdin(Stdio::from(slave.try_clone().expect("clone PTY stdin")))
            .stdout(Stdio::from(slave.try_clone().expect("clone PTY stdout")))
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("spawn PTY fixture");
        drop(slave);
        let input = master.try_clone().expect("clone PTY master");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(master).lines() {
                let Ok(line) = line else { break };
                if tx.send(line.trim_end_matches('\r').to_string()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            lines,
        }
    }

    fn recv_until(&self, prefix: &str, timeout: Duration) -> Result<String, RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(remaining)?;
            if line.starts_with(prefix) {
                return Ok(line);
            }
        }
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run_pty(
    program: &Path,
    is_node: bool,
    scenario: &str,
    order: &str,
    raw: bool,
) -> (ExitStatus, String) {
    let mut session = PtyChild::spawn(program, is_node, scenario, order, raw);
    session
        .recv_until("READY", Duration::from_secs(20))
        .unwrap_or_else(|_| panic!("{scenario}/{order}/raw={raw} never became ready"));
    session
        .input
        .write_all(if raw { b"ab\r" } else { b"ab\n" })
        .expect("write PTY input");
    session.input.flush().expect("flush PTY input");
    let line = session
        .recv_until("RESULT:", Duration::from_secs(5))
        .unwrap_or_else(|_| panic!("{scenario}/{order}/raw={raw} produced no result"));
    let result = line.trim_start_matches("RESULT:").to_string();

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = session.child.try_wait().expect("poll PTY fixture") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "PTY fixture did not exit: {result}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    (status, result)
}

#[test]
fn stdin_surface_matches_node_for_pipe_and_cooked_and_raw_ptys() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let (source, binary) = compile(dir.path());
    let cases = [("pipe", false), ("pty-cooked", false), ("pty-raw", true)];

    for scenario in ["keypress", "remove-data", "remove-all"] {
        for order in ["runtime-first", "readline-first"] {
            for (transport, raw) in cases {
                let run = |program: &Path, is_node: bool| {
                    if transport == "pipe" {
                        run_pipe(program, is_node, scenario, order)
                    } else {
                        run_pty(program, is_node, scenario, order, raw)
                    }
                };
                let (node_status, node) = run(&source, true);
                let (perry_status, perry) = run(&binary, false);
                assert!(
                    node_status.success(),
                    "Node oracle failed for {scenario}/{order}/{transport}: {node}"
                );
                assert!(
                    perry_status.success(),
                    "Perry failed for {scenario}/{order}/{transport}: {perry}"
                );
                assert_ne!(
                    node, "timeout",
                    "Node oracle timed out for {scenario}/{order}/{transport}"
                );
                assert_eq!(
                    perry, node,
                    "stdin divergence for {scenario}/{order}/{transport}"
                );
            }
        }
    }
}

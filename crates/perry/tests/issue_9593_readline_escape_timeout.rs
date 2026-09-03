//! Regression test for #9593: a torn raw-mode ESC prefix must use a real
//! `escapeCodeTimeout`, not "the next event-loop tick" as its clock.
//!
//! The child runs on a fresh PTY so `setRawMode(true)` exercises the production
//! stdin reader.  We cover the two loop schedules that exposed the defect:
//! with no other timer the old code delivered bare ESC at the 1 s idle cap,
//! while an unrelated 50 ms interval made it arrive after about 50 ms.  Node's
//! default is 500 ms in both cases.

#![cfg(unix)]

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"
import { emitKeypressEvents } from "node:readline";

emitKeypressEvents(process.stdin);
process.stdin.setRawMode(true);

if (process.env.PERRY_9593_INTERVAL === "1") {
  setInterval(() => {}, 50);
}

process.stdin.on("keypress", (_sequence, key) => {
  console.log("KEY:" + key.name);
});
console.log("READY");
"#;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write PTY fixture");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn open_pty() -> (File, File) {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0, "openpty failed: {}", std::io::Error::last_os_error());
    assert!(master >= 0 && slave >= 0);
    // SAFETY: openpty returned two fresh, owned descriptors above.
    unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
}

struct PtyChild {
    child: Child,
    input: File,
    lines: Receiver<(String, Instant)>,
}

impl PtyChild {
    fn spawn(program: &Path, arm_interval: bool) -> Self {
        let (master, slave) = open_pty();
        let child_stdin = slave.try_clone().expect("clone PTY slave for stdin");
        let child_stdout = slave.try_clone().expect("clone PTY slave for stdout");
        let mut command = Command::new(program);
        command
            .stdin(Stdio::from(child_stdin))
            .stdout(Stdio::from(child_stdout))
            .stderr(Stdio::null())
            .env("PERRY_9593_INTERVAL", if arm_interval { "1" } else { "0" });
        // Give the child its own session and make fd 0's PTY its controlling
        // terminal. The stdio descriptors have already been installed when
        // this hook runs.
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
        let child = command.spawn().expect("spawn PTY child");
        drop(slave);

        let input = master.try_clone().expect("clone PTY master for writes");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(master).lines() {
                match line {
                    Ok(line) => {
                        if tx
                            .send((line.trim_end_matches('\r').to_string(), Instant::now()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    // Linux returns EIO from a PTY master after the slave closes.
                    Err(_) => break,
                }
            }
        });

        let mut session = Self {
            child,
            input,
            lines,
        };
        let (ready, _) = session
            .recv_line(Duration::from_secs(30))
            .expect("PTY child never printed READY");
        assert_eq!(ready, "READY", "unexpected first PTY line");
        session
    }

    fn send(&mut self, bytes: &[u8]) {
        self.input.write_all(bytes).expect("write PTY input");
        self.input.flush().expect("flush PTY input");
    }

    fn recv_line(&mut self, timeout: Duration) -> Result<(String, Instant), RecvTimeoutError> {
        self.lines.recv_timeout(timeout)
    }

    fn recv_key(&mut self, timeout: Duration) -> (String, Instant) {
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            let (line, arrived) = self
                .recv_line(remaining)
                .unwrap_or_else(|_| panic!("no keypress line within {timeout:?}"));
            if let Some(name) = line.strip_prefix("KEY:") {
                return (name.to_string(), arrived);
            }
        }
    }

    fn assert_no_key_waiting(&mut self) {
        while let Ok((line, _)) = self.lines.try_recv() {
            assert!(
                !line.starts_with("KEY:"),
                "escape prefix fired before its completing bytes arrived: {line}"
            );
        }
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn bare_escape_latency(program: &Path, arm_interval: bool) -> Duration {
    let mut child = PtyChild::spawn(program, arm_interval);
    // Let the generated loop settle into its wait path before injecting input.
    std::thread::sleep(Duration::from_millis(100));
    let sent = Instant::now();
    child.send(b"\x1b");
    let (name, arrived) = child.recv_key(Duration::from_secs(2));
    assert_eq!(name, "escape");
    arrived.duration_since(sent)
}

#[test]
fn escape_timeout_is_clocked_and_split_arrows_still_coalesce() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let program = compile(dir.path());

    let idle_latency = bare_escape_latency(&program, false);
    let timer_latency = bare_escape_latency(&program, true);
    let expected_min = Duration::from_millis(250);
    let expected_max = Duration::from_millis(900);
    for (schedule, latency) in [("idle", idle_latency), ("50 ms interval", timer_latency)] {
        assert!(
            (expected_min..=expected_max).contains(&latency),
            "bare ESC under {schedule} was delivered after {latency:?}; expected the Node-compatible \
             500 ms escapeCodeTimeout, independent of the event-loop wait budget"
        );
    }
    assert!(
        idle_latency.abs_diff(timer_latency) < Duration::from_millis(250),
        "bare ESC changed with an unrelated timer: idle={idle_latency:?}, interval={timer_latency:?}"
    );

    let mut child = PtyChild::spawn(&program, false);
    child.send(b"\x1b");
    std::thread::sleep(Duration::from_millis(200));
    child.assert_no_key_waiting();
    let completed = Instant::now();
    child.send(b"[C");
    let (name, arrived) = child.recv_key(Duration::from_secs(1));
    assert_eq!(
        name, "right",
        "split arrow was torn into separate keypresses"
    );
    assert!(
        arrived.duration_since(completed) < Duration::from_millis(250),
        "split arrow was not delivered promptly after completion"
    );

    let sent = Instant::now();
    child.send(b"\x1b[C");
    let (name, arrived) = child.recv_key(Duration::from_secs(1));
    assert_eq!(name, "right");
    assert!(
        arrived.duration_since(sent) < Duration::from_millis(250),
        "complete arrow sequence should be delivered immediately"
    );
}

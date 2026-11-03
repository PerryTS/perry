//! A TTY `process.stdout` holds `columns` / `rows` as own data properties, read
//! through any value and refreshed before `SIGWINCH` listeners run (Node's
//! tty.WriteStream). Only the literal `process.stdout.columns` spelling used to
//! answer, so opentui (`config.stdout ?? process.stdout`) drew 80x24 in every
//! terminal.

#![cfg(unix)]

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-files/test_tty_write_stream_size.ts"
));

fn compile(dir: &Path) -> PathBuf {
    let source = dir.join("tty_size.ts");
    let binary = dir.join("tty_size_bin");
    std::fs::write(&source, SOURCE).expect("write tty fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile tty fixture");
    assert!(
        output.status.success(),
        "fixture compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

#[test]
fn tty_stdout_size_is_own_data_read_through_any_value() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let binary = compile(dir.path());

    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let mut size = winsize(179, 39);
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    assert_eq!(rc, 0, "openpty failed: {}", std::io::Error::last_os_error());
    // SAFETY: openpty returned two fresh owned descriptors.
    let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };

    let mut command = Command::new(&binary);
    command
        .env("PERRY_TTY_SIZE_DRIVE", "1")
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
    let mut child = command.spawn().expect("spawn PTY fixture");
    drop(slave);
    let reader = master.try_clone().expect("clone PTY master");
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            if tx.send(line.trim_end_matches('\r').to_string()).is_err() {
                break;
            }
        }
    });
    let next = |prefix: &str| loop {
        let line = lines
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("no {prefix} line"));
        if line.starts_with(prefix) {
            break line;
        }
    };

    assert_eq!(next("SIZE:"), "SIZE:179x39 param=179 own=true");
    next("READY");
    // Resizing the PTY sends SIGWINCH to its foreground process group.
    let resized = winsize(100, 30);
    let rc = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &resized) };
    assert_eq!(
        rc,
        0,
        "TIOCSWINSZ failed: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(next("RESIZED:"), "RESIZED:100x30");
    let _ = child.kill();
    let _ = child.wait();
}

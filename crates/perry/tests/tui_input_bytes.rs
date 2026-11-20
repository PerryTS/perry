#![cfg(unix)]

use std::error::Error;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

type TestResult<Value> = Result<Value, Box<dyn Error>>;

fn with_child<Value>(
    mut child: Child,
    action: impl FnOnce(&mut Child) -> TestResult<Value>,
) -> TestResult<Value> {
    let result = action(&mut child);
    let _ = child.kill();
    let reaped = child.wait();
    let value = result?;
    reaped?;
    Ok(value)
}

fn wait_for_ready(
    receiver: &mpsc::Receiver<Vec<u8>>,
    deadline: Instant,
    output: &mut Vec<u8>,
) -> TestResult<()> {
    while !output.windows(5).any(|part| part == b"READY") {
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "TUI did not render READY").into());
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(part) => output.extend(part),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn wait_for_exit(child: &mut Child, deadline: Instant) -> TestResult<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "TUI did not finish input").into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn input_fixture_timeouts_kill_and_reap_the_child() {
    for waiting_for_ready in [true, false] {
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let child_id = child.id() as libc::pid_t;
        assert_eq!(unsafe { libc::kill(child_id, 0) }, 0);
        let (_sender, receiver) = mpsc::channel();
        let error = with_child(child, |child| {
            if waiting_for_ready {
                wait_for_ready(&receiver, Instant::now(), &mut Vec::new())
            } else {
                wait_for_exit(child, Instant::now()).map(|_| ())
            }
        })
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            unsafe { libc::waitpid(child_id, std::ptr::null_mut(), libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}

#[test]
fn native_tui_input_bytes_preserves_split_utf8_and_paste() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.ts");
    let binary = directory.path().join("input");
    std::fs::write(
        &source,
        r#"
import { Text, run, exit, useInputBytes } from "perry/tui";
const expected = [228, 189, 160, 229, 165, 189, 240, 159, 152, 128,
    27, 91, 50, 48, 48, 126, 97, 9, 98, 27, 91, 50, 48, 49, 126, 0, 128, 255];
let received: number[] = [];
let saved: Uint8Array[] = [];
let valid = true;
run(() => {
    useInputBytes((chunk: Uint8Array) => {
        if (!(chunk instanceof Uint8Array)) valid = false;
        saved.push(chunk);
        for (let index = 0; index < chunk.length; index++) received.push(chunk[index]);
        if (received.length >= expected.length) exit();
    });
    return Text("READY");
});
let retained: number[] = [];
for (let part = 0; part < saved.length; part++) {
    for (let index = 0; index < saved[part].length; index++) retained.push(saved[part][index]);
}
console.log("RESULT", valid && JSON.stringify(received) === JSON.stringify(expected)
    && JSON.stringify(retained) === JSON.stringify(expected));
"#,
    )
    .unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .args(["compile"])
        .arg(&source)
        .arg("--no-auto-optimize")
        .arg("--no-cache")
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut master_fd = -1;
    let mut slave_fd = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let (mut master, slave) =
        unsafe { (File::from_raw_fd(master_fd), File::from_raw_fd(slave_fd)) };
    let mut before: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(slave_fd, &mut before) }, 0);
    let child = Command::new(&binary)
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()))
        .spawn()
        .unwrap();
    let (status, output) = with_child(child, |child| {
        let mut reader = master.try_clone()?;
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        })?;
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut output = Vec::new();
        wait_for_ready(&receiver, deadline, &mut output)?;
        let payload = [
            228, 189, 160, 229, 165, 189, 240, 159, 152, 128, 27, 91, 50, 48, 48, 126, 97, 9, 98,
            27, 91, 50, 48, 49, 126, 0, 128, 255,
        ];
        for byte in payload {
            master.write_all(&[byte])?;
            std::thread::sleep(Duration::from_millis(20));
        }
        let status = wait_for_exit(child, deadline)?;
        while let Ok(part) = receiver.recv_timeout(Duration::from_millis(100)) {
            output.extend(part);
        }
        Ok((status, output))
    })
    .unwrap();
    let mut after: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(slave_fd, &mut after) }, 0);
    assert_eq!(after.c_iflag, before.c_iflag);
    assert_eq!(after.c_oflag, before.c_oflag);
    assert_eq!(after.c_cflag, before.c_cflag);
    assert_eq!(
        after.c_lflag & !libc::PENDIN,
        before.c_lflag & !libc::PENDIN
    );
    assert_eq!(after.c_cc, before.c_cc);
    unsafe {
        assert_eq!(libc::cfgetispeed(&after), libc::cfgetispeed(&before));
        assert_eq!(libc::cfgetospeed(&after), libc::cfgetospeed(&before));
    }
    assert!(status.success());
    let output = String::from_utf8_lossy(&output);
    assert!(output.contains("RESULT true"), "{output}");
    assert!(output.contains("\x1b[?25h\x1b[?1049l"), "{output}");
}

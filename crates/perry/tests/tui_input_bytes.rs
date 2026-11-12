#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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
    let mut child = ChildGuard(
        Command::new(&binary)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()))
            .spawn()
            .unwrap(),
    );
    let mut reader = master.try_clone().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut output = Vec::new();
    while !output.windows(5).any(|part| part == b"READY") && Instant::now() < deadline {
        if let Ok(part) = receiver.recv_timeout(Duration::from_millis(50)) {
            output.extend(part);
        }
    }
    if !output.windows(5).any(|part| part == b"READY") {
        panic!("TUI did not render READY");
    }
    let payload = [
        228, 189, 160, 229, 165, 189, 240, 159, 152, 128, 27, 91, 50, 48, 48, 126, 97, 9, 98, 27,
        91, 50, 48, 49, 126, 0, 128, 255,
    ];
    for byte in payload {
        master.write_all(&[byte]).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            panic!("TUI did not finish input");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    while let Ok(part) = receiver.recv_timeout(Duration::from_millis(100)) {
        output.extend(part);
    }
    let mut after: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(slave_fd, &mut after) }, 0);
    assert_eq!(after.c_lflag, before.c_lflag);
    assert!(status.success());
    let output = String::from_utf8_lossy(&output);
    assert!(output.contains("RESULT true"), "{output}");
    assert!(output.contains("\x1b[?25h\x1b[?1049l"), "{output}");
}

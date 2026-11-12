#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::fd::FromRawFd;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn native_tui_advances_async_work_without_keyboard_input() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let port = server.local_addr().unwrap().port();
    let pending = TcpListener::bind("127.0.0.1:0").unwrap();
    let pending_port = pending.local_addr().unwrap().port();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("async.ts");
    let binary = directory.path().join("async");
    let fixture = r#"
import { Text, run, exit, useState } from "perry/tui";
let promiseDone = false;
let timeoutDone = false;
let intervalDone = false;
let httpDone = false;
let cancelDone = false;
let started = false;
let readyFrames = 0;
run(() => {
    const [version, setVersion] = useState(0);
    if (!started) {
        started = true;
        Promise.resolve().then(() => { promiseDone = true; setVersion(1); });
        setTimeout(() => { timeoutDone = true; setVersion(2); }, 10);
        const interval = setInterval(() => {
            intervalDone = true;
            clearInterval(interval);
            setVersion(3);
        }, 10);
        fetch("http://127.0.0.1:PORT/ready").then(response => response.text()).then(text => {
            httpDone = text === "ok";
            setVersion(4);
        });
        const controller = new AbortController();
        fetch("http://127.0.0.1:PENDING/hang", { signal: controller.signal }).catch(() => {
            cancelDone = true;
            setVersion(5);
        });
        setTimeout(() => controller.abort(), 20);
    }
    if (promiseDone && timeoutDone && intervalDone && httpDone && cancelDone) {
        readyFrames++;
        setTimeout(() => exit(), 0);
        return Text("READY1234");
    }
    return Text("waiting");
});
console.log("RESULT", promiseDone, timeoutDone, intervalDone, httpDone, cancelDone, readyFrames);
"#
    .replace("PENDING", &pending_port.to_string())
    .replace("PORT", &port.to_string());
    std::fs::write(&source, fixture).unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .arg("compile")
        .arg(&source)
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
    let (master, slave) = unsafe { (File::from_raw_fd(master_fd), File::from_raw_fd(slave_fd)) };
    let mut before: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(slave_fd, &mut before) }, 0);
    let mut child = Command::new(&binary)
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = master;
        let mut buffer = [0u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut served = false;
    let mut output = Vec::new();
    let status = loop {
        if let Ok((mut stream, _)) = server.accept() {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            served = true;
        }
        if let Ok(part) = receiver.recv_timeout(Duration::from_millis(10)) {
            output.extend(part);
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("async TUI stalled: {}", String::from_utf8_lossy(&output));
        }
    };
    while let Ok(part) = receiver.recv_timeout(Duration::from_millis(100)) {
        output.extend(part);
    }
    let mut after: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(slave_fd, &mut after) }, 0);
    assert_eq!(after.c_lflag, before.c_lflag);
    assert!(served, "HTTP fixture must actually receive a request");
    assert!(status.success());
    let output = String::from_utf8_lossy(&output);
    assert!(output.contains("READY1234"), "{output}");
    assert!(
        output.contains("RESULT true true true true true 1"),
        "{output}"
    );
    assert!(output.contains("\x1b[?25h\x1b[?1049l"), "{output}");
}

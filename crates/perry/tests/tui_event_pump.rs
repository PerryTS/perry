#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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

fn read_request(stream: &mut TcpStream, expected: &[u8]) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = vec![0; expected.len()];
    stream.read_exact(&mut request).unwrap();
    assert_eq!(request, expected);
}

#[test]
fn native_tui_advances_async_work_without_keyboard_input() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let port = server.local_addr().unwrap().port();
    let pending = TcpListener::bind("127.0.0.1:0").unwrap();
    pending.set_nonblocking(true).unwrap();
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
        fetch("http://127.0.0.1:PENDING/hang", { signal: controller.signal }).catch(error => {
            cancelDone = controller.signal.aborted && error.name === "AbortError";
            setVersion(5);
        });
        setTimeout(() => controller.abort(), 250);
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
    let (master, slave) = unsafe { (File::from_raw_fd(master_fd), File::from_raw_fd(slave_fd)) };
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
    let mut pending_request = None;
    let mut output = Vec::new();
    let status = loop {
        if let Ok((mut stream, _)) = server.accept() {
            read_request(&mut stream, b"GET /ready HTTP/1.1\r\n");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            served = true;
        }
        if let Ok((mut stream, _)) = pending.accept() {
            read_request(&mut stream, b"GET /hang HTTP/1.1\r\n");
            pending_request = Some(stream);
        }
        if let Ok(part) = receiver.recv_timeout(Duration::from_millis(10)) {
            output.extend(part);
        }
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.0.kill();
            let _ = child.0.wait();
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
    assert!(
        pending_request.is_some(),
        "aborted request must reach the server"
    );
    assert!(status.success());
    let output = String::from_utf8_lossy(&output);
    assert!(output.contains("READY1234"), "{output}");
    assert!(
        output.contains("RESULT true true true true true 1"),
        "{output}"
    );
    assert!(output.contains("\x1b[?25h\x1b[?1049l"), "{output}");
}

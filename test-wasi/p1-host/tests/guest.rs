//! Hand-written guests that exercise the host's preview 1 layer directly, so a
//! host defect is told apart from a runtime defect.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const HOST: &str = env!("CARGO_BIN_EXE_perry-p1-host");

fn module(name: &str, wat: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.wat"));
    std::fs::write(&path, wat).unwrap();
    path
}

fn stats(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}

const IMPORTS: &str = r#"
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close" (func $fd_close (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_fdstat_set_flags" (func $set_flags (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "poll_oneoff" (func $poll (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
  (import "wasi_snapshot_preview1" "environ_get" (func $environ_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_accept" (func $accept (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_recv" (func $recv (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_send" (func $send (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_shutdown" (func $shutdown (param i32 i32) (result i32)))
"#;

const ONE_PAGE: &str = r#"(memory (export "memory") 1)"#;
/// The most a 32-bit guest can have: the last byte is at 0xffff_ffff.
const FOUR_GIB: &str = r#"(memory (export "memory") 65536)"#;

#[test]
fn writes_stdout_sleeps_on_a_clock_and_exits_with_a_status() {
    let wat = format!(
        r#"(module {IMPORTS}{ONE_PAGE}
  (data (i32.const 100) "hi\n")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 100))
    (i32.store (i32.const 4) (i32.const 3))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
    (drop (call $prestat (i32.const 3) (i32.const 16)))
    (i64.store (i32.const 200) (i64.const 7))
    (i32.store (i32.const 216) (i32.const 1))
    (i64.store (i32.const 224) (i64.const 30000000))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400)))
    (call $exit (i32.add (i32.load (i32.const 400)) (i32.const 2)))))"#
    );
    let wasm = module("clock_exit", &wat);
    let stats_path = wasm.with_extension("json");
    let out = Command::new(HOST)
        .arg("--stats")
        .arg(&stats_path)
        .arg(&wasm)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(out.stdout, b"hi\n");
    let stats = stats(&stats_path);
    for expected in [
        "\"polls\":1",
        "\"clock_wakeups\":1",
        "\"fd_wakeups\":0",
        "\"prestat_calls\":1",
    ] {
        assert!(stats.contains(expected), "{expected} missing from {stats}");
    }
}

#[test]
fn echoes_one_connection_through_the_standard_socket_calls() {
    // fd 3 is the listener the host hands over. The guest makes it non-blocking,
    // waits for it in poll_oneoff, accepts, echoes once and exits with the
    // number of the accepted descriptor. It also checks that reading the
    // listener is refused for lack of the FD_READ right (76).
    let wat = format!(
        r#"(module {IMPORTS}{ONE_PAGE}
  (func (export "_start")
    (if (i32.ne (call $set_flags (i32.const 3) (i32.const 4)) (i32.const 0)) (then (call $exit (i32.const 90))))
    (i32.store (i32.const 0) (i32.const 1000))
    (i32.store (i32.const 4) (i32.const 64))
    (if (i32.ne (call $fd_read (i32.const 3) (i32.const 0) (i32.const 1) (i32.const 8)) (i32.const 76)) (then (call $exit (i32.const 91))))
    (i64.store (i32.const 200) (i64.const 1))
    (i32.store8 (i32.const 208) (i32.const 1))
    (i32.store (i32.const 216) (i32.const 3))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400)))
    (if (i32.ne (call $accept (i32.const 3) (i32.const 4) (i32.const 500)) (i32.const 0)) (then (call $exit (i32.const 92))))
    (i64.store (i32.const 200) (i64.const 2))
    (i32.store8 (i32.const 208) (i32.const 1))
    (i32.store (i32.const 216) (i32.load (i32.const 500)))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400)))
    (if (i32.ne (call $recv (i32.load (i32.const 500)) (i32.const 0) (i32.const 1) (i32.const 0) (i32.const 600) (i32.const 604)) (i32.const 0)) (then (call $exit (i32.const 93))))
    (i32.store (i32.const 4) (i32.load (i32.const 600)))
    (if (i32.ne (call $send (i32.load (i32.const 500)) (i32.const 0) (i32.const 1) (i32.const 0) (i32.const 608)) (i32.const 0)) (then (call $exit (i32.const 94))))
    (drop (call $shutdown (i32.load (i32.const 500)) (i32.const 3)))
    (drop (call $fd_close (i32.load (i32.const 500))))
    (call $exit (i32.load (i32.const 500)))))"#
    );
    let wasm = module("echo_once", &wat);
    let stats_path = wasm.with_extension("json");
    let mut host = Command::new(HOST)
        .args(["--listen", "127.0.0.1:0", "--stats"])
        .arg(&stats_path)
        .arg(&wasm)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(host.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    let addr = line
        .trim()
        .strip_prefix("listening ")
        .expect("listening line")
        .to_string();
    let mut client = TcpStream::connect(&addr).unwrap();
    client.write_all(b"ping").unwrap();
    let mut echoed = Vec::new();
    client.read_to_end(&mut echoed).unwrap();
    assert_eq!(echoed, b"ping");
    assert_eq!(host.wait().unwrap().code(), Some(4));
    assert!(stats(&stats_path).contains("\"accepts\":[4]"));
}

/// Starts the host with one listener and returns it with the listening address.
fn host_with_listener(wasm: &PathBuf) -> (std::process::Child, String) {
    let mut host = Command::new(HOST)
        .args(["--listen", "127.0.0.1:0"])
        .arg(wasm)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(host.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    let addr = line
        .trim()
        .strip_prefix("listening ")
        .expect("listening line")
        .to_string();
    (host, addr)
}

#[test]
fn poll_events_carry_the_ready_byte_count() {
    // A readable listener reports 1, a readable stream the queued bytes (4),
    // a writable stream something nonzero. Each failure exits with its own code.
    let wat = format!(
        r#"(module {IMPORTS}{ONE_PAGE}
  (func (export "_start")
    (drop (call $set_flags (i32.const 3) (i32.const 4)))
    (i64.store (i32.const 200) (i64.const 1))
    (i32.store8 (i32.const 208) (i32.const 1))
    (i32.store (i32.const 216) (i32.const 3))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400)))
    (if (i64.ne (i64.load (i32.const 316)) (i64.const 1)) (then (call $exit (i32.const 80))))
    (if (i32.ne (call $accept (i32.const 3) (i32.const 4) (i32.const 500)) (i32.const 0)) (then (call $exit (i32.const 81))))
    (i32.store (i32.const 216) (i32.load (i32.const 500)))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400)))
    (if (i64.ne (i64.load (i32.const 316)) (i64.const 4)) (then (call $exit (i32.const 82))))
    (i32.store8 (i32.const 208) (i32.const 2))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400)))
    (if (i64.eqz (i64.load (i32.const 316))) (then (call $exit (i32.const 83))))
    (call $exit (i32.const 0))))"#
    );
    let wasm = module("poll_nbytes", &wat);
    let (mut host, addr) = host_with_listener(&wasm);
    let mut client = TcpStream::connect(&addr).unwrap();
    client.write_all(b"ping").unwrap();
    assert_eq!(host.wait().unwrap().code(), Some(0));
}

#[test]
fn guest_pointers_that_overflow_or_leave_memory_fault_instead_of_panicking() {
    // With the largest memory a guest can have, 0xffff_fff8 is a valid address
    // but 48 bytes from it are not: the host must refuse, with FAULT (21), the
    // subscription array, the iovec array and an environment string that end
    // past the 32-bit space, not overflow while adding to them.
    let wat = format!(
        r#"(module {IMPORTS}{FOUR_GIB}
  (func (export "_start")
    (if (i32.ne (call $poll (i32.const 0xfffffff8) (i32.const 0) (i32.const 1) (i32.const 0)) (i32.const 21)) (then (call $exit (i32.const 1))))
    (if (i32.ne (call $poll (i32.const 0) (i32.const 0xfffffff0) (i32.const 1) (i32.const 0)) (i32.const 21)) (then (call $exit (i32.const 2))))
    (if (i32.ne (call $fd_write (i32.const 1) (i32.const 0xfffffffc) (i32.const 1) (i32.const 0)) (i32.const 21)) (then (call $exit (i32.const 3))))
    (if (i32.ne (call $environ_get (i32.const 0) (i32.const 0xfffffffd)) (i32.const 21)) (then (call $exit (i32.const 4))))
    (call $exit (i32.const 0))))"#
    );
    let wasm = module("pointer_overflow", &wat);
    let out = Command::new(HOST)
        .args(["--env", "A=b"])
        .arg(&wasm)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

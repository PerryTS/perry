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
  (import "perry_net" "tcp_connect" (func $tcp_connect (param i32 i32 i32) (result i32)))
  (import "perry_net" "sock_error" (func $sock_error (param i32 i32) (result i32)))
  (import "perry_net" "resolve" (func $resolve (param i32 i32 i32 i32) (result i32)))
"#;

const ONE_PAGE: &str = r#"(memory (export "memory") 1)"#;
/// The most a 32-bit guest can have: the last byte is at 0xffff_ffff.
const FOUR_GIB: &str = r#"(memory (export "memory") 65536)"#;

/// Helpers for the `perry_net` guests. Scratch memory: the descriptor out-param
/// at 500, `sock_error`'s value at 600, a subscription at 200 and its event at
/// 300, the event count at 400.
const NET_HELPERS: &str = r#"
  (func $wait (param $fd i32) (param $tag i32)
    (i64.store (i32.const 200) (i64.const 1))
    (i32.store8 (i32.const 208) (local.get $tag))
    (i32.store (i32.const 216) (local.get $fd))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400))))
  (func $connect (param $ptr i32) (param $len i32) (result i32)
    (if (i32.ne (call $tcp_connect (local.get $ptr) (local.get $len) (i32.const 500)) (i32.const 0))
      (then (call $exit (i32.const 81))))
    (i32.load (i32.const 500)))
  ;; The connection's state: 26 (the call's own EINPROGRESS) while undecided,
  ;; else the value written to the out-parameter.
  (func $error (param $fd i32) (result i32)
    (local $r i32)
    (local.set $r (call $sock_error (local.get $fd) (i32.const 600)))
    (if (i32.eq (local.get $r) (i32.const 26)) (then (return (i32.const 26))))
    (if (i32.ne (local.get $r) (i32.const 0)) (then (call $exit (i32.const 80))))
    (i32.load (i32.const 600)))
  (func $resolve_read (param $ptr i32) (param $len i32) (param $family i32) (result i32)
    (local $fd i32)
    (if (i32.ne (call $resolve (local.get $ptr) (local.get $len) (local.get $family) (i32.const 500)) (i32.const 0))
      (then (call $exit (i32.const 70))))
    (local.set $fd (i32.load (i32.const 500)))
    (call $wait (local.get $fd) (i32.const 1))
    (i32.store (i32.const 0) (i32.const 1100))
    (i32.store (i32.const 4) (i32.const 256))
    (if (i32.ne (call $fd_read (local.get $fd) (i32.const 0) (i32.const 1) (i32.const 8)) (i32.const 0))
      (then (call $exit (i32.const 71))))
    (drop (call $fd_close (local.get $fd)))
    (i32.load (i32.const 8)))
"#;

struct Run {
    status: Option<i32>,
    stats: String,
    elapsed: std::time::Duration,
}

/// Run a guest with `perry_net` helpers, `data` laid out from address 1000.
fn run_net(name: &str, host_args: &[&str], data: &str, start: &str) -> Run {
    let wat = format!(
        r#"(module {IMPORTS}{ONE_PAGE}{NET_HELPERS}
  (data (i32.const 1000) "{data}")
  (func (export "_start") {start}))"#
    );
    let wasm = module(name, &wat);
    let stats_path = wasm.with_extension("json");
    let began = std::time::Instant::now();
    let out = Command::new(HOST)
        .args(host_args)
        .arg("--stats")
        .arg(&stats_path)
        .arg(&wasm)
        .output()
        .unwrap();
    let elapsed = began.elapsed();
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Run {
        status: out.status.code(),
        stats: stats(&stats_path),
        elapsed,
    }
}

#[test]
fn a_connect_waits_for_a_slow_lookup_and_is_not_reported_ready_meanwhile() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let target = format!("slow.test:{}", listener.local_addr().unwrap().port());
    let start = format!(
        r#"(local $fd i32)
    (local.set $fd (call $connect (i32.const 1000) (i32.const {})))
    (if (i32.ne (call $error (local.get $fd)) (i32.const 26)) (then (call $exit (i32.const 82))))
    ;; The descriptor and a 50 ms clock: only the clock may fire.
    (i64.store (i32.const 200) (i64.const 1))
    (i32.store8 (i32.const 208) (i32.const 2))
    (i32.store (i32.const 216) (local.get $fd))
    (i64.store (i32.const 248) (i64.const 2))
    (i32.store (i32.const 264) (i32.const 1))
    (i64.store (i32.const 272) (i64.const 50000000))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 2) (i32.const 400)))
    (if (i32.ne (i32.load (i32.const 400)) (i32.const 1)) (then (call $exit (i32.const 83))))
    (if (i32.ne (i32.load8_u (i32.const 310)) (i32.const 0)) (then (call $exit (i32.const 83))))
    (if (i32.ne (call $error (local.get $fd)) (i32.const 26)) (then (call $exit (i32.const 84))))
    (call $wait (local.get $fd) (i32.const 2))
    (if (i32.ne (call $error (local.get $fd)) (i32.const 0)) (then (call $exit (i32.const 85))))
    (call $exit (i32.const 0))"#,
        target.len()
    );
    let run = run_net(
        "net_slow_dns",
        &["--dns", "slow.test=127.0.0.1,delay=300"],
        &target,
        &start,
    );
    assert_eq!(run.status, Some(0), "{}", run.stats);
    assert!(run.elapsed >= std::time::Duration::from_millis(300));
    assert!(
        run.stats.contains("\"connect_attempts\":1"),
        "{}",
        run.stats
    );
}

#[test]
fn a_lookup_failure_is_reported_after_its_delay_without_a_connect() {
    let start = r#"(local $fd i32)
    (local.set $fd (call $connect (i32.const 1000) (i32.const 11)))
    (call $wait (local.get $fd) (i32.const 2))
    (if (i32.ne (call $error (local.get $fd)) (i32.const 2000)) (then (call $exit (i32.const 82))))
    (if (i32.ne (call $error (local.get $fd)) (i32.const 2000)) (then (call $exit (i32.const 83))))
    (call $exit (i32.const 0))"#;
    let run = run_net(
        "net_notfound",
        &["--dns", "gone.test=notfound,delay=100"],
        "gone.test:9",
        start,
    );
    assert_eq!(run.status, Some(0), "{}", run.stats);
    assert!(run.elapsed >= std::time::Duration::from_millis(100));
    assert!(
        run.stats.contains("\"connect_attempts\":0"),
        "{}",
        run.stats
    );
}

#[test]
fn closing_a_connect_during_the_lookup_cancels_it_and_frees_the_number() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let literal = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let data = format!("slow.test:80{literal}");
    // The guest outlives the cancelled lookup's 200 ms delay, then sends on the
    // connection that reused the number: the late answer must not disturb it.
    let start = format!(
        r#"(local $first i32)
    (local.set $first (call $connect (i32.const 1000) (i32.const 12)))
    (if (i32.ne (call $fd_close (local.get $first)) (i32.const 0)) (then (call $exit (i32.const 82))))
    (if (i32.ne (call $fd_close (local.get $first)) (i32.const 8)) (then (call $exit (i32.const 83))))
    (if (i32.ne (call $connect (i32.const 1012) (i32.const {})) (local.get $first)) (then (call $exit (i32.const 84))))
    (call $wait (local.get $first) (i32.const 2))
    (if (i32.ne (call $error (local.get $first)) (i32.const 0)) (then (call $exit (i32.const 85))))
    ;; Sleep 400 ms on a clock.
    (i64.store (i32.const 200) (i64.const 7))
    (i32.store8 (i32.const 208) (i32.const 0))
    (i32.store (i32.const 216) (i32.const 1))
    (i64.store (i32.const 224) (i64.const 400000000))
    (drop (call $poll (i32.const 200) (i32.const 300) (i32.const 1) (i32.const 400)))
    (if (i32.ne (call $error (local.get $first)) (i32.const 0)) (then (call $exit (i32.const 86))))
    (i32.store (i32.const 0) (i32.const 1000))
    (i32.store (i32.const 4) (i32.const 9))
    (if (i32.ne (call $send (local.get $first) (i32.const 0) (i32.const 1) (i32.const 0) (i32.const 608)) (i32.const 0)) (then (call $exit (i32.const 87))))
    (if (i32.ne (i32.load (i32.const 608)) (i32.const 9)) (then (call $exit (i32.const 88))))
    (drop (call $fd_close (local.get $first)))
    (call $exit (i32.const 0))"#,
        literal.len()
    );
    let run = run_net(
        "net_cancel",
        &["--dns", "slow.test=127.0.0.1,delay=200"],
        &data,
        &start,
    );
    assert_eq!(run.status, Some(0), "{}", run.stats);
    assert!(run.elapsed >= std::time::Duration::from_millis(400));
    // Only the literal reached the network, and its bytes arrived intact.
    assert!(
        run.stats.contains("\"connect_attempts\":1"),
        "{}",
        run.stats
    );
    let mut received = Vec::new();
    listener
        .accept()
        .unwrap()
        .0
        .read_to_end(&mut received)
        .unwrap();
    assert_eq!(received, b"slow.test");
}

#[test]
fn an_out_parameter_outside_memory_is_a_fault_that_leaks_no_descriptor() {
    // Both imports refuse the out-parameter, 0xffff_fffc, before doing
    // anything: no connect starts, no resolver runs, and the next descriptor is
    // still the first free number (3).
    let start = r#"
    (if (i32.ne (call $tcp_connect (i32.const 1000) (i32.const 11) (i32.const 0xfffffffc)) (i32.const 21)) (then (call $exit (i32.const 82))))
    (if (i32.ne (call $resolve (i32.const 1011) (i32.const 9) (i32.const 0) (i32.const 0xfffffffc)) (i32.const 21)) (then (call $exit (i32.const 83))))
    (if (i32.ne (call $connect (i32.const 1000) (i32.const 11)) (i32.const 3)) (then (call $exit (i32.const 84))))
    (call $exit (i32.const 0))"#;
    let run = run_net("net_fault", &[], "127.0.0.1:9slow.test", start);
    assert_eq!(run.status, Some(0), "{}", run.stats);
    assert!(
        run.stats.contains("\"connect_attempts\":1"),
        "{}",
        run.stats
    );
    assert!(run.stats.contains("\"connects\":[3]"), "{}", run.stats);
}

#[test]
fn a_resolved_address_that_refuses_keeps_its_own_errno() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    for (name, host) in [
        ("net_refused_name", "refuse.test"),
        ("net_refused_ip", "127.0.0.1"),
    ] {
        let target = format!("{host}:{port}");
        let start = format!(
            r#"(local $fd i32)
    (local.set $fd (call $connect (i32.const 1000) (i32.const {})))
    (call $wait (local.get $fd) (i32.const 2))
    (if (i32.ne (call $error (local.get $fd)) (i32.const 14)) (then (call $exit (i32.const 82))))
    (call $exit (i32.const 0))"#,
            target.len()
        );
        let run = run_net(
            name,
            &["--dns", "refuse.test=127.0.0.1,delay=20"],
            &target,
            &start,
        );
        assert_eq!(run.status, Some(0), "{name}: {}", run.stats);
    }
}

// 127.0.0.2 is loopback on Linux only, and refuses there because the listener
// is bound to 127.0.0.1.
#[cfg(target_os = "linux")]
#[test]
fn the_addresses_are_tried_in_order_until_one_connects() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let target = format!("multi.test:{}", listener.local_addr().unwrap().port());
    let start = format!(
        r#"(local $fd i32)
    (local.set $fd (call $connect (i32.const 1000) (i32.const {})))
    (call $wait (local.get $fd) (i32.const 2))
    (if (i32.ne (call $error (local.get $fd)) (i32.const 0)) (then (call $exit (i32.const 82))))
    (call $exit (i32.const 0))"#,
        target.len()
    );
    let run = run_net(
        "net_order",
        &["--dns", "multi.test=127.0.0.2+127.0.0.1"],
        &target,
        &start,
    );
    assert_eq!(run.status, Some(0), "{}", run.stats);
    assert!(
        run.stats.contains("\"connect_attempts\":2"),
        "{}",
        run.stats
    );
}

#[test]
fn resolve_streams_a_status_byte_then_records_filtered_by_family() {
    // "multi.test" at 1000 (10 bytes), "v4.test" at 1010 (7 bytes).
    let start = r#"
    (if (i32.ne (call $resolve_read (i32.const 1000) (i32.const 10) (i32.const 0)) (i32.const 35)) (then (call $exit (i32.const 90))))
    (if (i32.ne (i32.load8_u (i32.const 1100)) (i32.const 0)) (then (call $exit (i32.const 91))))
    (if (i32.ne (i32.load8_u (i32.const 1101)) (i32.const 4)) (then (call $exit (i32.const 92))))
    (if (i32.ne (i32.load8_u (i32.const 1102)) (i32.const 127)) (then (call $exit (i32.const 93))))
    (if (i32.ne (i32.load8_u (i32.const 1118)) (i32.const 6)) (then (call $exit (i32.const 94))))
    (if (i32.ne (call $resolve_read (i32.const 1000) (i32.const 10) (i32.const 6)) (i32.const 18)) (then (call $exit (i32.const 95))))
    (if (i32.ne (i32.load8_u (i32.const 1101)) (i32.const 6)) (then (call $exit (i32.const 96))))
    ;; Nothing of the family asked for is a name not found: status 1 alone.
    (if (i32.ne (call $resolve_read (i32.const 1010) (i32.const 7) (i32.const 6)) (i32.const 1)) (then (call $exit (i32.const 97))))
    (if (i32.ne (i32.load8_u (i32.const 1100)) (i32.const 1)) (then (call $exit (i32.const 98))))
    (call $exit (i32.const 0))"#;
    let run = run_net(
        "net_resolve",
        &[
            "--dns",
            "multi.test=127.0.0.1+::1,delay=100",
            "--dns",
            "v4.test=127.0.0.1",
        ],
        "multi.testv4.test",
        start,
    );
    assert_eq!(run.status, Some(0), "{}", run.stats);
    assert!(run.elapsed >= std::time::Duration::from_millis(100));
}

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

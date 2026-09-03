//! Regression test for #9588: the stdin reader thread must WAKE the main
//! event-loop pump after queueing input.
//!
//! `perry-stdlib`'s readline reader (`readline::ensure_reader_started`) is a
//! cross-thread producer: it blocks in `read(2)` on its own thread and pushes
//! what it reads into `PENDING_DATA` / `PENDING_LINES`, queues that only the
//! MAIN thread drains — from `js_readline_process_pending`, reached through
//! `js_run_stdlib_pump` once per event-loop turn.
//!
//! It used to push and go straight back into `read(2)` without calling
//! `js_notify_main_thread()`. The main loop was therefore never told the input
//! existed: it learned about a keystroke only when it happened to wake for some
//! *other* reason. `js_wait_for_event` sizes that sleep from the timer
//! deadlines, so the delivery latency was whatever the next timer was — and for
//! a program sitting idle waiting for input, with no timer armed at all, the
//! full `IDLE_CAP_MS` (1 s) safety cap.
//!
//! Measured on the claude-code bundle before the fix: 695-963 ms from keypress
//! to the `'data'` handler (Node: sub-millisecond). Every other cross-thread
//! producer in the tree already follows the protocol the event pump documents
//! ("producer: push_to_queue(); js_notify_main_thread()") — the child-process
//! reactor, the pty reactor, dgram, signals, and perry-runtime's OWN stdin
//! reader in `os_process_streams`. readline's reader was the one that skipped
//! it.
//!
//! The test measures from the parent: it writes one chunk after the child is
//! known to be parked, and times how long the child takes to echo it back. It
//! fails on the unfixed reader (~700 ms+) and passes on the fixed one (~1 ms).
//! Deliberately NO timers in the fixture — a single `setInterval` would mask
//! the bug completely by capping the park at the interval period, which is
//! exactly why the defect survived so long in programs that happen to have one.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// No timers, no intervals: the only live handle is the stdin listener, so the
/// event loop parks for the idle cap between turns.
const SOURCE: &str = r#"
process.stdin.on("data", (chunk: any) => {
  console.log("DATA:" + String(chunk).trim());
});
console.log("READY");
"#;

fn compile(dir: &std::path::Path, source: &str) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
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

#[test]
fn stdin_reader_wakes_the_event_loop_instead_of_waiting_for_the_idle_cap() {
    let dir = std::env::temp_dir().join(format!("perry_9588_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let bin = compile(&dir, SOURCE);

    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn compiled binary");

    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");

    // Reader thread: hand each complete line back with the instant it arrived.
    let (tx, rx) = mpsc::channel::<(String, Instant)>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send((l, Instant::now())).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let ready = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("child never printed READY");
    assert_eq!(ready.0.trim(), "READY", "unexpected first line");

    // Let the loop settle into its park. With no timer armed the budget is the
    // full idle cap, so from here nothing but a notify can wake it early.
    std::thread::sleep(Duration::from_millis(400));

    let mut latencies = Vec::new();
    for i in 0..3 {
        let sent = Instant::now();
        writeln!(stdin, "chunk{i}").expect("write stdin");
        stdin.flush().expect("flush stdin");
        let (line, arrived) = rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("no 'data' delivery for chunk{i} within 10 s"));
        assert_eq!(line.trim(), format!("DATA:chunk{i}"), "wrong chunk echoed");
        latencies.push(arrived.duration_since(sent));
        // Re-park between chunks so each measurement starts from a fresh sleep.
        std::thread::sleep(Duration::from_millis(300));
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    // The unfixed reader lands at the idle cap (~700 ms-1 s). The fixed one is
    // a wake plus one loop turn — single-digit milliseconds. 250 ms sits an
    // order of magnitude clear of both.
    let worst = latencies.iter().max().copied().unwrap();
    assert!(
        worst < Duration::from_millis(250),
        "stdin delivery took {worst:?} (all: {latencies:?}) — the reader queued the \
         input without calling js_notify_main_thread, so the main loop slept out \
         its js_wait_for_event budget before draining it"
    );
}

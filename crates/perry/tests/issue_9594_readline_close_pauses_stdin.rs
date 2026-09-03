//! Regression test for #9594: closing a stdin-backed readline interface must
//! pause the shared `process.stdin` stream.
//!
//! The readline interface and `process.stdin` share one native reader. Perry
//! used to mark the interface closed without pausing that reader, so bytes
//! written after `rl.close()` still reached a `process.stdin` `data` listener.
//! Node pauses stdin on both `rl.close()` and `rl.pause()`. The stream remains
//! usable: an explicit `process.stdin.resume()` after close enables delivery
//! again.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
import * as readline from "readline";

const mode = process.argv[2];
const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false,
});

rl.question("", (_answer: string) => {
  const reportLateData = (chunk: string) => {
    console.log("LATE_DATA:" + JSON.stringify(String(chunk)));
  };

  if (mode === "close-no-listener") {
    rl.close();
  } else if (mode === "close-listener-before") {
    process.stdin.on("data", reportLateData);
    rl.close();
  } else if (mode === "pause") {
    process.stdin.on("data", reportLateData);
    rl.pause();
  } else if (mode === "close-listener-after") {
    rl.close();
    process.stdin.on("data", reportLateData);
  } else if (mode === "close-resume") {
    rl.close();
    process.stdin.on("data", reportLateData);
    process.stdin.on("end", () => console.log("STDIN_END"));
    process.stdin.resume();
  }

  console.log("READY_FOR_LATE");
  setTimeout(() => console.log("DONE"), 150);
});

console.log("READY_FOR_ANSWER");
"#;

fn compile(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
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

fn recv_until(rx: &Receiver<String>, expected: &str, output: &mut Vec<String>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let line = rx
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("child never printed {expected}; output: {output:?}"));
        let matched = line == expected;
        output.push(line);
        if matched {
            return;
        }
    }
}

fn wait_for_exit(child: &mut Child, mode: &str, output: &[String]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait().expect("poll compiled fixture") {
            Some(status) => {
                assert!(
                    status.success(),
                    "{mode} fixture exited with {status}; output: {output:?}"
                );
                return;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{mode} fixture did not exit after stdin closed; output: {output:?}");
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn run_arm(bin: &Path, mode: &str) -> Vec<String> {
    let mut child = Command::new(bin)
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn compiled fixture");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut output = Vec::new();
    recv_until(&rx, "READY_FOR_ANSWER", &mut output);
    writeln!(stdin, "answer").expect("write answer");
    stdin.flush().expect("flush answer");
    recv_until(&rx, "READY_FOR_LATE", &mut output);
    writeln!(stdin, "late").expect("write late input");
    stdin.flush().expect("flush late input");
    drop(stdin);
    recv_until(&rx, "DONE", &mut output);
    wait_for_exit(&mut child, mode, &output);
    reader.join().expect("join stdout reader");
    while let Ok(line) = rx.try_recv() {
        output.push(line);
    }
    output
}

#[test]
fn readline_close_matches_stdin_pause_without_permanently_muting_stdin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(dir.path());

    for mode in [
        "close-no-listener",
        "close-listener-before",
        "pause",
        "close-listener-after",
    ] {
        let output = run_arm(&bin, mode);
        assert!(
            output.iter().all(|line| !line.starts_with("LATE_DATA:")),
            "{mode} delivered bytes after stdin was paused: {output:?}"
        );
    }

    let resumed = run_arm(&bin, "close-resume");
    assert!(
        resumed.iter().any(|line| line == "LATE_DATA:\"late\\n\""),
        "explicit resume after close did not restore stdin delivery: {resumed:?}"
    );
    assert!(
        resumed.iter().any(|line| line == "STDIN_END"),
        "closing readline suppressed process.stdin's later EOF: {resumed:?}"
    );
}

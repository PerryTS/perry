//! Regression coverage for OpenCode's child-process writable path (#8512).
//!
//! OpenCode adapts `child.stdin` through Effect's `NodeSink.fromWritable` for
//! LSP framing and process pipelines. That adapter waits for Node's optional
//! `write` / `end` completion callbacks. Perry used to perform the pipe write
//! but drop those callbacks, leaving the sink (and therefore the LSP or
//! formatter lifecycle) pending forever.

#![cfg(unix)]

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const FIXTURE: &str = r#"
import { spawn } from "node:child_process";

async function completion(register: (done: () => void) => void): Promise<boolean> {
  let synchronous = true;
  const asynchronous = await new Promise<boolean>((resolve) => {
    register(() => resolve(!synchronous));
    synchronous = false;
  });
  return asynchronous;
}

async function main() {
  const childCwd = process.env.OPENCODE_CHILD_CWD as string;
  const child = spawn("sh", ["-c", 'printf "%s\\n%s\\n" "$PWD" "$OPENCODE_LSP"; cat'], {
    cwd: childCwd,
    env: { PATH: process.env.PATH, OPENCODE_LSP: "ready" },
    stdio: ["pipe", "pipe", "pipe"],
  });

  let stdout = "";
  child.stdout!.on("data", (chunk: Buffer) => {
    stdout += chunk.toString("utf8");
  });
  const closed = new Promise<{ code: number | null; signal: string | null }>((resolve) => {
    child.once("close", (code: number | null, signal: string | null) => resolve({ code, signal }));
  });

  const shortWriteAsync = await completion((done) => child.stdin!.write("frame-one\n", done));
  const encodedWriteAsync = await completion((done) => child.stdin!.write("frame-two\n", "utf8", done));
  const endAsync = await completion((done) => child.stdin!.end(done));
  const result = await closed;

  console.log("WRITE_CB_ASYNC:" + shortWriteAsync);
  console.log("ENCODED_WRITE_CB_ASYNC:" + encodedWriteAsync);
  console.log("END_CB_ASYNC:" + endAsync);
  console.log("EXIT:" + result.code + ":" + result.signal);
  console.log("CWD:" + stdout.startsWith(childCwd + "\n"));
  console.log("ENV:" + stdout.includes("\nready\n"));
  console.log("FRAMES:" + stdout.includes("frame-one\nframe-two\n"));
}

main();
"#;

#[test]
fn opencode_lsp_writable_callbacks_complete() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    let child_cwd = dir.path().join("child-cwd");
    std::fs::create_dir(&child_cwd).expect("create distinct child cwd");
    let child_cwd = std::fs::canonicalize(child_cwd).expect("canonicalize child cwd");
    std::fs::write(&entry, FIXTURE).expect("write fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // A dropped completion callback leaves `cat` alive with stdin open, so use
    // a hard timeout to turn that historical hang into a deterministic test
    // failure.
    let mut child = Command::new(&output)
        .current_dir(dir.path())
        .env("OPENCODE_CHILD_CWD", &child_cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run compiled fixture");
    let mut stdout_pipe = child.stdout.take().expect("stdout pipe");
    let mut stderr_pipe = child.stderr.take().expect("stderr pipe");
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_pipe.read_to_string(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        buf
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll compiled fixture") {
            break status;
        }
        if start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = stdout_reader.join().expect("stdout reader");
            let stderr = stderr_reader.join().expect("stderr reader");
            panic!(
                "compiled fixture hung waiting for a child stdin callback:\nstdout: {stdout}\nstderr: {stderr}"
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };

    let stdout = stdout_reader.join().expect("stdout reader");
    let stderr = stderr_reader.join().expect("stderr reader");
    assert!(
        status.success(),
        "compiled fixture failed ({status:?}):\nstdout: {stdout}\nstderr: {stderr}"
    );
    for expected in [
        "WRITE_CB_ASYNC:true",
        "ENCODED_WRITE_CB_ASYNC:true",
        "END_CB_ASYNC:true",
        "EXIT:0:null",
        "CWD:true",
        "ENV:true",
        "FRAMES:true",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in {stdout:?}"
        );
    }
}

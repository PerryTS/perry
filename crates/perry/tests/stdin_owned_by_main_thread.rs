//! fd 0 belongs to the main thread. Perry's stdin byte buffer and listener
//! lists are process-global, and every agent's event loop used to drain them:
//! a worker whose loop kept turning took the main thread's input and called the
//! main thread's `data` listener on the worker thread. A TUI (opencode on
//! macOS) lost keystrokes and terminal query replies at random and froze.
//!
//! The fixture's worker spins its loop with `setImmediate`, so on an unpatched
//! build nearly every chunk is taken by the worker; patched, the main thread
//! counts every byte. Both registration spellings are covered: the aliased
//! stream (the runtime's listener lists) and literal `process.stdin.on` (the
//! readline registry codegen lowers it to).

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-files/test_stdin_owned_by_main_thread.ts"
));
const WORKER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-files/_helpers/stdin_owner_spin_worker.ts"
));

const CHUNKS: usize = 200;
const CHUNK_BYTES: usize = 512;
const DEADLINE: Duration = Duration::from_secs(30);

fn compile(dir: &Path) -> PathBuf {
    let source = dir.join("stdin_owner.ts");
    let binary = dir.join("stdin_owner_bin");
    std::fs::create_dir_all(dir.join("_helpers")).expect("create helper directory");
    std::fs::write(&source, SOURCE).expect("write stdin fixture");
    std::fs::write(dir.join("_helpers/stdin_owner_spin_worker.ts"), WORKER)
        .expect("write worker helper");
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile stdin fixture");
    assert!(
        output.status.success(),
        "fixture compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn run(binary: &Path, spelling: &str) -> String {
    let mut child = Command::new(binary)
        .env("PERRY_STDIN_OWNER_DRIVE", "1")
        .env("PERRY_STDIN_OWNER_SPELLING", spelling)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn stdin fixture");
    let stdout = child.stdout.take().expect("fixture stdout");
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let ready = lines.recv_timeout(DEADLINE);
    assert_eq!(
        ready.as_deref(),
        Ok("READY"),
        "{spelling}: fixture never became ready"
    );
    let mut stdin = child.stdin.take().expect("fixture stdin");
    let piece = [b'x'; CHUNK_BYTES];
    for _ in 0..CHUNKS {
        if stdin.write_all(&piece).is_err() {
            break;
        }
        stdin.flush().expect("flush fixture stdin");
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(stdin);

    let started = Instant::now();
    let mut result = None;
    while started.elapsed() < DEADLINE {
        match lines.recv_timeout(Duration::from_millis(50)) {
            Ok(line) if line.starts_with("RESULT:") => {
                result = Some(line);
                break;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll stdin fixture") {
            break Some(status);
        }
        if started.elapsed() > DEADLINE {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = child.kill();
    let _ = child.wait();
    let result = result.unwrap_or_else(|| panic!("{spelling}: no RESULT line (exit {status:?})"));
    assert!(
        status.is_some_and(|s| s.success()),
        "{spelling}: fixture did not exit cleanly: {status:?} ({result})"
    );
    result
}

#[test]
fn a_spinning_worker_does_not_take_the_main_threads_stdin() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let binary = compile(dir.path());
    for spelling in ["alias", "literal"] {
        for round in 0..3 {
            assert_eq!(
                run(&binary, spelling),
                format!("RESULT:{}", CHUNKS * CHUNK_BYTES),
                "{spelling} round {round}: bytes piped to the main thread were lost"
            );
        }
    }
}

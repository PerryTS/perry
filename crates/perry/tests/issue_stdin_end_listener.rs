//! Regression test: `process.stdin.once("end", …)` (and `on("end"/"close", …)`)
//! must fire on stdin EOF.
//!
//! The native `process.stdin` `on`/`once` binding only registered `"data"` and
//! `"readable"` listeners — `"end"`/`"close"` fell into a `_ => {}` arm and were
//! silently dropped. A prompt reader that races an EOF against a timeout
//! (`p = new Promise(res => { stdin.once("end", () => res(false)); … })`, then
//! `await p`) therefore never resolved: the `'end'` listener never fired, the
//! awaiter hung, and the program stalled before doing any further work.
//!
//! Fix: `process_stdin_on`/`process_stdin_once` register `"end"`/`"close"`
//! listeners in dedicated registries that the main-thread stdin pump fires once
//! the reader hits EOF and the byte buffer has drained.
//!
//! Needs the default (auto-optimize) build — the native stdin reader/pump lives
//! in perry-runtime and drives the event loop.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

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

/// Run `bin`, writing `stdin_bytes` then closing stdin (EOF). Fails if the
/// process doesn't exit within `secs` (the regression is a hang: `'end'` never
/// fires, so the awaiter never resolves).
fn run_with_stdin(bin: &std::path::Path, stdin_bytes: &[u8], secs: u64) -> String {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn compiled binary");

    // Write the input and drop the handle so the child sees EOF.
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin_bytes)
        .expect("write stdin");

    let mut piped = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = piped.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let stdout = reader.join().unwrap_or_default();
                assert!(
                    status.success(),
                    "binary exited non-zero: {status:?}\nstdout:\n{stdout}"
                );
                return stdout;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = reader.join().unwrap_or_default();
                panic!(
                    "regression: process hung >{secs}s — process.stdin 'end' \
                     listener never fired.\nstdout so far:\n{stdout}"
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// `once("end", …)` alongside a persistent `on("data", …)` (flowing mode) must
/// fire after the piped input is consumed — the prompt-reader shape.
#[test]
fn stdin_once_end_fires_after_data() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
let acc = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (c: string) => { acc += c; });
process.stdin.once("end", () => {
  process.stdout.write("END " + JSON.stringify(acc) + "\n");
  process.exit(0);
});
// A regression manifests as this line (or a harness timeout) instead of END.
setTimeout(() => { process.stdout.write("NO-END\n"); process.exit(0); }, 6000);
"#,
    );
    let stdout = run_with_stdin(&bin, b"hello world\n", 20);
    assert_eq!(stdout, "END \"hello world\\n\"\n");
}

/// EOF with no input at all (`< /dev/null` shape): `once("end")` must still fire.
#[test]
fn stdin_once_end_fires_on_empty_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
process.stdin.setEncoding("utf8");
process.stdin.on("data", () => {});
process.stdin.once("end", () => {
  process.stdout.write("END-EMPTY\n");
  process.exit(0);
});
setTimeout(() => { process.stdout.write("NO-END\n"); process.exit(0); }, 6000);
"#,
    );
    let stdout = run_with_stdin(&bin, b"", 20);
    assert_eq!(stdout, "END-EMPTY\n");
}

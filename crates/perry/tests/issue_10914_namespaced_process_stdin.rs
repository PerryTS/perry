//! A `node:process` namespace import must register literal stdin listeners.

#![cfg(unix)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn namespaced_process_stdin_on_receives_data_and_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import * as process from "node:process";

function shadowed() {
  const process: any = { stdin: { on(_event: string, _handler: any) { return "local"; } } };
  return process.stdin.on("data", () => {});
}
console.log("SHADOW", shadowed());

let total = 0, chunks = 0, ended = false;
process.stdin.on("data", (c: Uint8Array) => { total += c.length; chunks++; });
process.stdin.on("end", () => {
  ended = true;
  console.log("END", total, chunks > 0);
});
process.on("exit", (code: number) => {
  console.log("EXIT", code, ended, total, chunks > 0);
});
"#,
    )
    .expect("write fixture");

    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run fixture");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"hello")
        .expect("write stdin");
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.try_wait().expect("poll fixture").is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("collect timed-out fixture");
            panic!(
                "stdin fixture timed out: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("collect fixture output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("SHADOW local"), "{stdout}");
    assert!(stdout.contains("END 5 true"), "{stdout}");
    assert!(stdout.contains("EXIT 0 true 5 true"), "{stdout}");
}

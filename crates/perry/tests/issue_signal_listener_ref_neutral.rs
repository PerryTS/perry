//! A registered `process.on('SIGINT'|'SIGTERM'|…)` listener must be **ref-neutral**:
//! per Node, installing a signal listener does NOT keep the event loop alive on its
//! own (the docs' example calls `process.stdin.resume()` precisely because the
//! listener alone won't keep the process running). Pre-fix, perry's
//! `has_active_process_signal_listeners()` returned true for any slot with
//! `listeners > 0`, so a CLI that installs SIGINT/SIGTERM handlers at startup (the
//! universal graceful-shutdown pattern) idled forever after its real work drained —
//! the program hung instead of exiting. This test installs several signal handlers,
//! does trivial work, and asserts the program EXITS on its own (if the bug is
//! present, the compiled binary never exits and this test times out).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn signal_listeners_do_not_pin_the_event_loop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let count = 0;
process.on("SIGINT", () => { count++; });
process.on("SIGTERM", () => { count++; });
process.on("SIGHUP", () => { count++; });
// Some async work that completes, then nothing else keeps the loop alive
// except the (ref-neutral) signal listeners. The program must exit on its own.
setTimeout(() => {
  console.log("work done, handlers:", count);
  console.log("DONE");
}, 10);
"#,
    )
    .expect("write entry");

    let output = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    // If the signal listeners wrongly pinned the loop, this run would never return.
    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("DONE"), "expected DONE, got:\n{stdout}");
    assert!(
        stdout.contains("work done, handlers: 0"),
        "expected handlers count 0, got:\n{stdout}"
    );
}

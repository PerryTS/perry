//! Regression test for same-thread `MessageChannel`/`MessagePort` message
//! delivery. Previously `MessagePort.postMessage` was a no-op and `onmessage`
//! was a plain null field, so messages were silently dropped — a program using
//! a `port1.onmessage = cb; port2.postMessage(0)` macrotask-scheduler pattern
//! would idle forever because the scheduled callback never ran.
//!
//! Now `postMessage` structured-clones the value, enqueues it on the entangled
//! port, and schedules a macrotask that dispatches a `MessageEvent`-shaped
//! object to the paired port's `onmessage` handler and any
//! `addEventListener("message", ...)` listeners, in FIFO order, one task each.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

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
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// `onmessage` on one port receives a `postMessage` from the entangled port.
#[test]
fn onmessage_receives_post_from_entangled_port() {
    let stdout = compile_and_run(
        r#"
const mc = new MessageChannel();
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 3000);
mc.port1.onmessage = (e: any) => {
  console.log("got", e.data, e.type);
  process.exit(0);
};
mc.port2.postMessage("ping");
"#,
    );
    assert_eq!(stdout, "got ping message\n");
}

/// Multiple messages deliver in FIFO order, plus the macrotask-scheduler
/// pattern (a handler that re-posts) runs to completion.
#[test]
fn fifo_order_and_scheduler_pattern() {
    let stdout = compile_and_run(
        r#"
const mc = new MessageChannel();
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 3000);
const got: number[] = [];
let count = 0;
mc.port1.onmessage = (e: any) => {
  got.push(e.data);
  count++;
  if (count < 4) {
    mc.port2.postMessage(count + 1);
  } else {
    console.log(got.join(","));
    process.exit(0);
  }
};
mc.port2.postMessage(1);
"#,
    );
    assert_eq!(stdout, "1,2,3,4\n");
}

/// A message posted before `onmessage` is assigned still delivers once the
/// handler is installed.
#[test]
fn message_queued_before_start_delivers_on_assign() {
    let stdout = compile_and_run(
        r#"
const mc = new MessageChannel();
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 3000);
mc.port2.postMessage("early");
setTimeout(() => {
  mc.port1.onmessage = (e: any) => {
    console.log("late", e.data);
    process.exit(0);
  };
}, 50);
"#,
    );
    assert_eq!(stdout, "late early\n");
}

/// `addEventListener("message", fn)` works alongside `onmessage`.
#[test]
fn add_event_listener_and_onmessage_both_fire() {
    let stdout = compile_and_run(
        r#"
const mc = new MessageChannel();
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 3000);
let viaListener = false;
let viaOnmessage = false;
const done = () => {
  if (viaListener && viaOnmessage) { console.log("both"); process.exit(0); }
};
mc.port1.addEventListener("message", (e: any) => { viaListener = true; done(); });
mc.port1.onmessage = (e: any) => { viaOnmessage = true; done(); };
mc.port2.postMessage("x");
"#,
    );
    assert_eq!(stdout, "both\n");
}

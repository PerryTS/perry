//! Regression test for #4913 (Stage 2): `Atomics.wait`/`notify`/`waitAsync`
//! must really block and wake across `perry/thread` agents over a shared
//! `SharedArrayBuffer`, instead of the pre-fix non-blocking fakes (`wait`
//! always `"timed-out"`, `notify` always `0`, `waitAsync` resolves
//! `"timed-out"` immediately).
//!
//! Three things are exercised:
//!   1. Real blocking — `wait` on a matching value with no notifier blocks for
//!      ~the timeout (the old stub returned immediately).
//!   2. Cross-thread shared memory + `notify` waking a parked waiter — a worker
//!      stores into a SAB the main thread also views and wakes the parked
//!      `wait`; the store is visible on the main thread (real aliasing).
//!   3. `waitAsync` — its promise resolves `"ok"` on a cross-thread `notify`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// `Atomics.wait` with a matching value and no notifier must actually block
/// for ~the timeout (then return `"timed-out"`). Pre-fix it returned instantly.
#[test]
fn wait_really_blocks_until_timeout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const sab = new SharedArrayBuffer(4);
const a = new Int32Array(sab);
const t0 = Date.now();
const r = Atomics.wait(a, 0, 0, 120);
const elapsed = Date.now() - t0;
console.log("result", r);
console.log("blocked", elapsed >= 100);
"#,
    );
    assert_eq!(
        stdout, "result timed-out\nblocked true\n",
        "Atomics.wait must block for the full timeout, not return immediately"
    );
}

/// A `SharedArrayBuffer` captured into a `spawn` closure aliases the same bytes
/// across the thread boundary: the worker's `Atomics.store` is visible on the
/// main thread, and its `Atomics.notify` wakes the main thread's parked `wait`
/// (the worker naps first so the main thread parks before the notify).
#[test]
fn shared_memory_aliases_and_notify_wakes_parked_wait() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { spawn } from "perry/thread";
const sab = new SharedArrayBuffer(8);
const main = new Int32Array(sab);
const worker = spawn(() => {
  const view = new Int32Array(sab);
  const nap = new Int32Array(new SharedArrayBuffer(4));
  Atomics.wait(nap, 0, 0, 100); // sleep so the main thread parks first
  Atomics.store(view, 0, 42);
  return Atomics.notify(view, 0);
});
const status = Atomics.wait(main, 0, 0, 10000);
const value = Atomics.load(main, 0);
const woke = await worker;
console.log("status", status);
console.log("value", value);
console.log("woke", woke);
"#,
    );
    assert_eq!(
        stdout, "status ok\nvalue 42\nwoke 1\n",
        "the worker's store must be visible on the main thread and its notify \
         must wake the parked wait (status ok, woke 1)"
    );
}

/// `Atomics.waitAsync` returns `{ async: true, value: Promise }`; the promise
/// resolves `"ok"` when another agent notifies. The waiter is enqueued
/// synchronously at the call (before the worker spawns), so the notify can't
/// be missed — a deterministic `"ok"`.
#[test]
fn wait_async_resolves_on_cross_thread_notify() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { spawn } from "perry/thread";
const sab = new SharedArrayBuffer(4);
const a = new Int32Array(sab);
const r = Atomics.waitAsync(a, 0, 0, 10000) as { async: boolean; value: Promise<string> };
console.log("async", r.async);
spawn(() => {
  const view = new Int32Array(sab);
  const nap = new Int32Array(new SharedArrayBuffer(4));
  Atomics.wait(nap, 0, 0, 30);
  return Atomics.notify(view, 0);
});
const result = await r.value;
console.log("result", result);
"#,
    );
    assert_eq!(
        stdout, "async true\nresult ok\n",
        "Atomics.waitAsync must return an async promise that resolves 'ok' on \
         a cross-thread notify"
    );
}

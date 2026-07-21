//! Regression test for #6709: `await` on a *pending* Promise inside an
//! `async function*` (async generator) must SUSPEND the generator and return
//! control to the caller — not busy-wait.
//!
//! ## Root cause
//!
//! `crates/perry-transform/src/async_to_generator.rs` — the pass that rewrites
//! every `await` into a state-machine suspend point (`await` → yield → an
//! `AsyncStepChain` continuation via `Promise.resolve(x).then(step)`, so the
//! function returns a pending Promise and the rest of the body runs on the
//! microtask queue) — is a **no-op for generators**:
//!
//! ```ignore
//! if !func.is_async || func.is_generator {   // async_to_generator.rs:89
//!     return;
//! }
//! ```
//!
//! So an `async function*` keeps its `Expr::Await` nodes un-rewritten, and they
//! lower via the *busy-wait poll loop* in
//! `crates/perry-codegen/src/expr/fs_await.rs` (`js_await_any_promise` +
//! `js_promise_run_microtasks_await_loop` + `js_await_loop_tick_timers`, spun
//! until the awaited Promise settles).
//!
//! That poll loop never returns to the caller. When the awaited Promise is
//! *pending* and can only be settled by code that runs **after** `.next()`
//! returns (the classic push/pull async-iterator: the generator awaits
//! `new Promise(r => this.waiting.push(r))` and a later `push()` calls `r`),
//! `.next()` busy-waits forever on a Promise nobody can resolve → **deadlock**.
//! The event loop then drains and prints "Detected unsettled top-level await".
//!
//! Already-resolved awaits (`await Promise.resolve(x)`) and yield-only async
//! generators work — the poll loop sees the Promise settled immediately — so
//! this is specific to *pending* awaits.
//!
//! ## Impact
//!
//! The push/pull async-iterator shape below (a generator `await`ing
//! `new Promise(r => waiting.push(r))`, resolved by a later `push()`) is how
//! pi's interactive TUI wires agent → session → UI events (its hand-rolled
//! `EventStream`), so this deadlock is on that path. NOTE: fixing this alone
//! does NOT restore pi's interactivity — an *earlier* keypress→submit
//! divergence (#6728) means Enter never fires submit under perry, so the
//! EventStream never runs regardless. This test targets the async-generator
//! deadlock in isolation, which is a real bug in its own right.
//!
//! ## The fix (landed in this PR)
//!
//! `async function*` bodies now suspend on `await` like plain async functions:
//! the generator state machine is split at `await` points (distinct from
//! consumer `yield` points) and driven through the existing `AsyncStepChain`
//! async-step machinery, so `.next()` returns a pending Promise immediately and
//! resumes on the microtask queue when the awaited Promise settles. This
//! touched the async generator lowering (`generator/lower.rs`,
//! `generator/linearize.rs`) + the `Expr::Yield` discriminator, validated
//! against the full async-generator test262 suite.
//!
//! With the fix landed the test is active (no `#[ignore]`); it uses a hard
//! timeout so a regression fails deterministically rather than hanging.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Minimal push/pull async-iterator (the shape pi's `EventStream` uses): an
/// async generator that `await`s a Promise resolved by a later `push()`.
const FIXTURE: &str = r#"
const log = (m) => process.stderr.write(m + "\n");
class EventStream {
  queue = []; waiting = []; done = false;
  push(event) {
    if (this.done) return;
    const waiter = this.waiting.shift();
    if (waiter) { waiter({ value: event, done: false }); }
    else { this.queue.push(event); }
  }
  end() { this.done = true; while (this.waiting.length > 0) { const w = this.waiting.shift(); w({ value: void 0, done: true }); } }
  async *[Symbol.asyncIterator]() {
    while (true) {
      if (this.queue.length > 0) { yield this.queue.shift(); }
      else if (this.done) { return; }
      else {
        const result = await new Promise((resolve) => this.waiting.push(resolve));
        if (result.done) return;
        yield result.value;
      }
    }
  }
}
async function main() {
  const s = new EventStream();
  const consumer = (async () => { for await (const e of s) { log("GOT " + e.type); } })();
  await Promise.resolve(); await Promise.resolve();
  s.push({ type: "agent_start" });
  s.push({ type: "turn_start" });
  await Promise.resolve(); await Promise.resolve();
  s.push({ type: "message_start" });
  await Promise.resolve(); await Promise.resolve();
  s.end();
  await consumer;
  log("done");
}
main().then(() => log("main:resolved"));
"#;

/// Node v26 output (stderr), for reference:
///   GOT agent_start
///   GOT turn_start
///   GOT message_start
///   done
///   main:resolved
const EXPECTED: &str = "GOT agent_start\nGOT turn_start\nGOT message_start\ndone\nmain:resolved\n";

/// A user `return X` sitting in a yield/await-free control-flow block that
/// precedes an `await` lands in the `StateExit::Await` state's body (the
/// linearizer's catch-all accumulates it into `current`, which the next `await`
/// takes as the state body). That return must settle the async generator's
/// result Promise as an iter-result completion — `.next()` must resolve to
/// `{value: X, done: true}`, not to the bare value `X`. Regression guard for the
/// CodeRabbit finding on #6727 (the `StateExit::Await` arm skipped the
/// `prepend_done_before_returns` + `rewrite_returns_as_done` rewrite the
/// `Yield`/`Goto`/`Done` arms apply, so the step closure escaped with a raw
/// `return X`).
const RETURN_BEFORE_AWAIT_FIXTURE: &str = r#"
async function* g(cond) {
  if (cond) { return 5; }
  await Promise.resolve();
  yield 1;
}
async function main() {
  const it = g(true);
  console.log("A " + JSON.stringify(await it.next()));
  console.log("B " + JSON.stringify(await it.next()));
  const it2 = g(false);
  console.log("C " + JSON.stringify(await it2.next()));
  console.log("D " + JSON.stringify(await it2.next()));
}
main();
"#;

/// Node v26 output (stdout):
///   A {"value":5,"done":true}
///   B {"done":true}
///   C {"value":1,"done":false}
///   D {"done":true}
const RETURN_BEFORE_AWAIT_EXPECTED: &str =
    "A {\"value\":5,\"done\":true}\nB {\"done\":true}\nC {\"value\":1,\"done\":false}\nD {\"done\":true}\n";

#[test]
fn async_generator_return_before_await_settles_as_iter_result() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.mjs");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, RETURN_BEFORE_AWAIT_FIXTURE).unwrap();

    let status = Command::new(perry_bin())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .status()
        .expect("perry compile");
    assert!(status.success(), "compile failed");

    let out = Command::new(&output).output().expect("run");
    assert!(out.status.success(), "binary exited non-zero");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout, RETURN_BEFORE_AWAIT_EXPECTED,
        "async generator `return` before an `await` diverged from node"
    );
}

#[test]
fn async_generator_pending_await_suspends_not_deadlocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.mjs");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, FIXTURE).unwrap();

    let status = Command::new(perry_bin())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .status()
        .expect("perry compile");
    assert!(status.success(), "compile failed");

    // Run with a hard timeout — the buggy runtime deadlocks, so poll for exit
    // and drain stderr on a reader thread (a pipe can't be read after the child
    // is reaped by `try_wait`).
    let mut child = Command::new(&output)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stderr_pipe = child.stderr.take().expect("stderr pipe");
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        buf
    });
    let start = Instant::now();
    let mut deadlocked = false;
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            deadlocked = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let stderr = reader.join().expect("reader join");
    assert!(
        !deadlocked,
        "async generator deadlocked (never exited) — pending await busy-waited; got so far: {stderr:?}"
    );
    assert_eq!(
        stderr, EXPECTED,
        "async generator output diverged from node"
    );
}

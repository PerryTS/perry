//! Regression test for #10854: a worker's `onmessage` handler that awaits
//! anything never resumed, so the reply was never posted.
//!
//! After the worker's module body ran, the thread parked in a blocking receive
//! and invoked the JS handler straight from there, then parked again. Nothing
//! drained the microtask queue, so an `async` handler — which returns a pending
//! promise — never got past its first `await`. The message was received and
//! silently never answered: no rejection, no exception, no exit.
//!
//! This is the ordinary shape for a request/response worker protocol, and it is
//! what left OpenCode's TUI painting nothing: its `Rpc.listen` does
//! `const result = await rpc[parsed.method](parsed.input)` before `postMessage`,
//! so every request the TUI made was received and none was answered.
//!
//! The pump must drain microtasks/nextTicks ONLY. `timer.rs` keeps
//! `TIMER_QUEUE`/`CALLBACK_TIMERS`/`INTERVAL_TIMERS` in global mutexes rather
//! than thread-locals, so an `AllowTimers` drain here runs the MAIN thread's
//! timer callbacks on the worker thread against the worker's globals — that
//! corrupted the main thread nondeterministically (a later main-thread timer
//! died with "value is not a function"). The two `await` shapes below are the
//! ones a microtask drain must cover; a sync handler is included because it
//! worked before the fix and must keep working.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

/// Awaits a microtask, then an async function call — OpenCode's exact shape.
const WORKER_SOURCE: &str = r#"
// @ts-nocheck
const rpc = { async echo(x) { return "echo:" + x } }
onmessage = async (e) => {
  await Promise.resolve()
  const result = await rpc.echo(e.data)
  postMessage(result)
}
"#;

/// A sync handler in a second worker: this path worked before the fix and the
/// pump must not disturb it.
const SYNC_WORKER_SOURCE: &str = r#"
// @ts-nocheck
onmessage = (e) => {
  postMessage("sync:" + e.data)
}
"#;

const MAIN_SOURCE: &str = r#"
// @ts-nocheck
// Literal specifiers: perry must resolve a Worker entry at COMPILE time.
const seen = []
const asyncWorker = new Worker("./worker_async.ts", { type: "module" })
const syncWorker = new Worker("./worker_sync.ts", { type: "module" })

function done() {
  if (seen.length < 2) return
  seen.sort()
  console.log(seen.join("\n"))
  process.exit(0)
}
asyncWorker.onmessage = (e) => { seen.push("1 " + e.data); done() }
syncWorker.onmessage = (e) => { seen.push("2 " + e.data); done() }
asyncWorker.postMessage("a")
syncWorker.postMessage("b")

setTimeout(() => {
  console.log("TIMEOUT waiting for the worker reply; got: " + JSON.stringify(seen))
  process.exit(1)
}, 15000)
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "1 echo:a\n2 sync:b\n";

#[test]
fn worker_async_onmessage_resumes_after_await() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("worker_async.ts"), WORKER_SOURCE).unwrap();
    std::fs::write(root.join("worker_sync.ts"), SYNC_WORKER_SOURCE).unwrap();
    std::fs::write(root.join("main.ts"), MAIN_SOURCE).unwrap();

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(root)
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "pre-fix this timed out: the async handler received the message and never \
         replied\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, EXPECTED,
        "an async onmessage handler must resume after its await and post its reply"
    );
}

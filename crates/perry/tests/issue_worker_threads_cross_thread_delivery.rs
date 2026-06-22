//! Regression test for cross-thread `worker_threads.Worker` execution and
//! message delivery.
//!
//! Three previously-broken behaviours are covered:
//!
//! 1. **`addEventListener` on the Worker handle and on `parentPort`.** Both
//!    objects only exposed the Node-style `on`/`once`/`off`; the Web-style
//!    `addEventListener("message", ...)` form (which a program using the
//!    EventTarget surface relies on) tripped the unimplemented-API gate at
//!    compile time and threw `addEventListener is not a function` / a deferred
//!    "not implemented" error at runtime. The listener now fires with a
//!    `MessageEvent` (carrying `.data`).
//!
//! 2. **Every spawned worker runs its entry.** A worker target module is
//!    compiled to an idempotent `<prefix>__init` wrapper guarded by a
//!    process-global "init done" flag. The first worker ran the body and set
//!    the flag; every subsequent worker saw it set and returned immediately,
//!    so only one worker of a pool ever executed — the rest idled and the
//!    parent waited forever. The spawn path now calls the unguarded
//!    `__init_body` so each worker thread (with its own arena) runs its entry.
//!
//! 3. **Bidirectional messaging.** worker -> main (`worker.on`/
//!    `addEventListener("message")`) and main -> worker
//!    (`parentPort.on`/`addEventListener("message")`).

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `main.ts` (which references the sibling worker file by relative
/// path) together with every file already written into `dir`, run it, and
/// return its stdout. Asserts both compile and run succeed.
fn compile_and_run(dir: &Path, main_src: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, main_src).expect("write entry");

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

/// One worker, bidirectional round trip using the Web-style `addEventListener`
/// surface on both the Worker handle and `parentPort`.
#[test]
fn add_event_listener_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("child.ts"),
        r#"
import { parentPort, workerData } from "worker_threads";
parentPort!.addEventListener("message", (ev: any) => {
  parentPort!.postMessage({ ack: ev.data });
});
parentPort!.postMessage({ hello: workerData });
"#,
    )
    .expect("write child");

    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Worker } from "worker_threads";
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 5000);
let n = 0;
const w = new Worker("./child.ts", { workerData: 42 });
w.addEventListener("message", (ev: any) => {
  const d = ev.data;
  n++;
  if (n === 1) {
    console.log("from-worker", d.hello);
    w.postMessage({ ping: 7 });
  } else {
    console.log("ack", d.ack.ping);
    w.terminate().then(() => { console.log("done"); process.exit(0); });
  }
});
"#,
    );
    assert_eq!(stdout, "from-worker 42\nack 7\ndone\n");
}

/// A pool of four workers: every worker must run its entry and report back,
/// exercising the unguarded-`__init_body` per-worker execution fix. The pattern
/// mirrors a parallel search that splits a range across worker threads.
#[test]
fn worker_pool_all_workers_execute() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("search.ts"),
        r#"
import { parentPort, workerData } from "worker_threads";
const { lo, hi, needle } = workerData as { lo: number; hi: number; needle: number };
let found = -1;
for (let i = lo; i < hi; i++) {
  if (i * i === needle) { found = i; break; }
}
parentPort!.postMessage({ found });
"#,
    )
    .expect("write search worker");

    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Worker } from "worker_threads";
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 8000);
const cores = 4;
const total = 100;
const per = Math.ceil(total / cores);
const needle = 49; // 7 * 7
let done = 0;
let answer = -1;
const workers: Worker[] = [];
for (let c = 0; c < cores; c++) {
  const lo = c * per;
  const hi = Math.min(lo + per, total);
  const w = new Worker("./search.ts", { workerData: { lo, hi, needle } });
  workers.push(w);
  w.on("message", (msg: any) => {
    if (msg.found >= 0) answer = msg.found;
    done++;
    if (done === cores) {
      console.log("workers", done, "answer", answer);
      Promise.all(workers.map((x) => x.terminate())).then(() => {
        console.log("done");
        process.exit(answer === 7 ? 0 : 3);
      });
    }
  });
}
"#,
    );
    assert_eq!(stdout, "workers 4 answer 7\ndone\n");
}

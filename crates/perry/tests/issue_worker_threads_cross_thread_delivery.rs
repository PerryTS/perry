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

/// A source-file Web Worker discovered through the canonical module-URL
/// spelling. Neither side imports `worker_threads`: construction, worker-scope
/// globals, property handlers, reload, structured values, env, close, and
/// terminate all use the browser/Bun surface while still compiling to native
/// entry functions.
#[test]
fn global_web_worker_module_url_rpc_reload_and_close() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("web-worker.ts"),
        r#"
postMessage({ kind: "ready", token: process.env.WEB_WORKER_TOKEN });
let propertyPayload = 0;
onmessage = (ev: any) => {
  propertyPayload = ev.data.payload.rpc;
};
const listener = (ev: any) => {
  removeEventListener("message", listener);
  postMessage({
    kind: "reply",
    payload: ev.data.payload,
    propertyPayload,
    nested: [1, { ok: true }],
  });
  close();
};
addEventListener("message", listener);
"#,
    )
    .expect("write web worker");

    let stdout = compile_and_run(
        dir.path(),
        r#"
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 8000);
const worker = new Worker(new URL("./web-worker.ts", import.meta.url), {
  type: "module",
  env: { WEB_WORKER_TOKEN: "web-env" },
});
let ready = 0;
worker.onmessage = (ev: any) => {
  if (ev.data.kind === "ready") {
    ready++;
    console.log("ready", ready, ev.data.token);
    if (ready === 1) worker.reload();
    else worker.postMessage({ payload: { rpc: 8509 } });
    return;
  }
  console.log("reply", ev.data.payload.rpc, ev.data.propertyPayload, ev.data.nested[1].ok);
  worker.terminate().then(() => { console.log("done"); process.exit(0); });
};
"#,
    );
    assert_eq!(
        stdout,
        "ready 1 web-env\nready 2 web-env\nreply 8509 8509 true\ndone\n"
    );
}

/// OpenCode mixes the browser/Bun Worker surface in the parent with Node's
/// `parentPort` surface in OpenTUI's parser worker. Its RPC payloads also carry
/// byte arrays, and its TUI server worker receives a complete `process.env`
/// snapshot. Exercise that exact boundary plus the SIGUSR2 reload and SIGINT
/// shutdown sequence rather than testing the two Worker API shapes only in
/// isolation.
#[cfg(unix)]
#[test]
fn global_worker_interops_with_parent_port_and_uint8array() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("parser-worker.ts"),
        r#"
import { isMainThread, parentPort } from "node:worker_threads";

parentPort!.on("message", (message: any) => {
  if (message.method === "reload" || message.method === "shutdown") {
    parentPort!.postMessage({
      type: "rpc.result",
      id: message.id,
      result: message.method,
    });
    return;
  }
  const input = message.input as Uint8Array;
  parentPort!.postMessage({
    type: "rpc.result",
    id: message.id,
    env: process.env.OPENCODE_WORKER_TOKEN,
    workerThread: !isMainThread,
    inputBrand: input instanceof Uint8Array,
    payload: new Uint8Array([input[2], input[1], input[0], 255]),
  });
});
parentPort!.postMessage({ type: "ready" });
"#,
    )
    .expect("write parser worker");

    let stdout = compile_and_run(
        dir.path(),
        r#"
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 8000);
process.env.OPENCODE_WORKER_TOKEN = "copied-env";
const worker = new Worker(new URL("./parser-worker.ts", import.meta.url), {
  env: Object.fromEntries(
    Object.entries(process.env).filter((entry): entry is [string, string] => entry[1] !== undefined),
  ),
});
const reload = () => worker.postMessage({ type: "rpc.request", method: "reload", id: 10104 });
const shutdown = () => worker.postMessage({ type: "rpc.request", method: "shutdown", id: 10105 });
process.on("SIGUSR2", reload);
process.on("SIGINT", shutdown);
worker.onerror = (event: any) => {
  console.log("worker-error", event.message);
  process.exit(3);
};
worker.on("exit", (code: number) => console.log("exit", code));
worker.onmessage = (event: any) => {
  const message = event.data;
  if (message.type === "ready") {
    worker.postMessage({
      type: "rpc.request",
      method: "roundTrip",
      id: 10103,
      input: new Uint8Array([3, 5, 8]),
    });
    return;
  }
  if (message.id === 10104) {
    console.log("signal", message.result);
    process.kill(process.pid, "SIGINT");
    return;
  }
  if (message.id === 10105) {
    console.log("signal", message.result);
    process.off("SIGUSR2", reload);
    process.off("SIGINT", shutdown);
    worker.terminate().then((code: number) => {
      console.log("terminated", code);
      process.exit(0);
    });
    return;
  }
  const bytes = message.payload as Uint8Array;
  console.log(
    "reply",
    message.id,
    message.env,
    message.workerThread,
    message.inputBrand,
    bytes instanceof Uint8Array,
    bytes.length,
    bytes[0],
    bytes[1],
    bytes[2],
    bytes[3],
  );
  process.kill(process.pid, "SIGUSR2");
};
"#,
    );
    assert_eq!(
        stdout,
        "reply 10103 copied-env true true true 4 8 5 3 255\nsignal reload\nsignal shutdown\nexit 1\nterminated 1\n"
    );
}

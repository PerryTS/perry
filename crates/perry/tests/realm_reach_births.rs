//! The program's realm R emits exactly the runtime layouts its runtime paths
//! can birth (A1, #13783). Each case below is a birth path whose layout R
//! once missed, so the program aborted with "runtime layout X is born, but the
//! program's realm does not reach it"; it now runs, and its binary defines
//! the layout. The negatives are programs that cannot reach the builtin: their
//! binaries define none of those layouts, so R did not grow for everyone.
//! A probe (is this object or prototype on layout X?) compares X's fixed word
//! and names no descriptor, so a program without X never aborts asking.
//! Internal runtime scheduling (a zlib or fs completion, AbortSignal.timeout)
//! never goes through a JS-visible builder: it births no Timeout/Immediate
//! handle, so those programs run without the TIMEOUT/IMMEDIATE layouts, in
//! the same loop phase and order as node, as node's async_hooks providers.
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// A compiled fixture that does not exit within `limit` fails the test
/// (killed, with what it printed) instead of blocking the suite.
fn run_with_timeout(mut command: Command, limit: Duration) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("run compiled binary");
    let deadline = Instant::now() + limit;
    while child.try_wait().expect("poll compiled binary").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out binary");
            let output = child.wait_with_output().expect("collect timed out output");
            panic!(
                "compiled binary did not exit within {limit:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    child.wait_with_output().expect("collect binary output")
}

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    if let Some(runtime) = std::env::var_os("PERRY_RUNTIME_DIR") {
        return PathBuf::from(runtime);
    }
    perry_bin()
        .parent()
        .expect("compiler directory")
        .to_path_buf()
}

/// Compile `source` with symbols kept, run it, and return its stdout, the
/// runtime layout descriptors the binary defines, and the run's wall time.
fn compile_and_run(dir: &Path, source: &str) -> (String, BTreeSet<String>, Duration) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_KEEP_SYMBOLS", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let started = Instant::now();
    let mut command = Command::new(&output);
    command.current_dir(dir);
    let run = run_with_timeout(command, Duration::from_secs(60));
    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success() && !stderr.contains("is born, but the program's realm"),
        "compiled binary failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        stderr
    );
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        layouts(&output),
        elapsed,
    )
}

/// Defined `perry_runtime_layout_<NAME>` descriptors (not the key lists).
fn layouts(binary: &Path) -> BTreeSet<String> {
    let nm = Command::new("nm").arg(binary).output().expect("run nm");
    assert!(nm.status.success(), "nm failed");
    let all: BTreeSet<String> = String::from_utf8_lossy(&nm.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace().rev();
            let name = fields.next()?;
            let kind = fields.next()?;
            (kind != "U" && kind != "w")
                .then(|| {
                    name.strip_prefix("perry_runtime_layout_")
                        .map(str::to_string)
                })
                .flatten()
        })
        .collect();
    assert!(
        all.contains("OBJECT_PROTOTYPE"),
        "the symbol read must see the every-program layouts: {all:?}"
    );
    all
}

fn case(source: &str, expected: &str, layout: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, defined, _) = compile_and_run(dir.path(), source);
    assert_eq!(stdout, expected, "output of the {layout} witness");
    assert!(
        defined.contains(layout),
        "{layout} is not emitted: {defined:?}"
    );
}

/// A runtime-internal completion: the program runs without the timer handle
/// layouts (no JS-visible Timeout/Immediate is born for it).
fn internal_case(source: &str, expected: &str) -> Duration {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, defined, elapsed) = compile_and_run(dir.path(), source);
    assert_eq!(
        stdout, expected,
        "output of the internal-scheduling witness"
    );
    for absent in ["TIMEOUT", "IMMEDIATE"] {
        assert!(!defined.contains(absent), "{absent} emitted: {defined:?}");
    }
    elapsed
}

#[test]
fn zlib_callback_forms_birth_no_timer_handle() {
    internal_case(
        r#"import { gzip, gunzip } from "node:zlib";
gzip(Buffer.from("perry"), (e: any, z: any) => gunzip(z, (e2: any, out: any) => console.log(String(out))));
"#,
        "perry\n",
    );
}

#[test]
fn fs_operations_birth_no_timer_handle() {
    internal_case(
        r#"import { writeFile, readFile } from "node:fs/promises";
import { writeFile as writeFileCb, createReadStream } from "node:fs";
const path = process.cwd() + "/out.txt";
await writeFile(path, "a");
console.log(await readFile(path, "utf8"));
writeFileCb(path, "b", () => createReadStream(path).on("data", (d: any) => console.log(String(d))));
"#,
        "a\nb\n",
    );
}

/// The signal fires while ref'd work (the user's timeout) keeps the loop.
#[test]
fn abort_signal_timeout_fires_while_the_loop_runs() {
    case(
        r#"const s = AbortSignal.timeout(1);
s.addEventListener("abort", () => console.log("aborted", s.aborted));
setTimeout(() => console.log("done"), 50);
"#,
        "aborted true\ndone\n",
        "TIMEOUT",
    );
}

/// Receipt (unref): a pending `AbortSignal.timeout(10000)` does not hold the
/// loop, as in node. (Before, the timer id handed to unref was the handle
/// pointer, so the program waited the full 10 s.)
#[test]
fn a_pending_abort_signal_timeout_does_not_hold_the_loop() {
    let elapsed = internal_case(
        r#"const s = AbortSignal.timeout(10000);
s.addEventListener("abort", () => console.log("aborted"));
console.log("end");
"#,
        "end\n",
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "held the loop for {elapsed:?}"
    );
}

/// Receipt (ref): internal completions keep the loop alive until they run;
/// nothing else in this program does.
#[test]
fn internal_completions_keep_the_loop_alive() {
    internal_case(
        r#"import { writeFile } from "node:fs";
import { gzip } from "node:zlib";
const seen: string[] = [];
writeFile(process.cwd() + "/ref.txt", "y", () => seen.push("fs"));
gzip(Buffer.from("q"), () => seen.push("zlib"));
process.on("exit", () => console.log(seen.sort().join(",")));
"#,
        "fs,zlib\n",
    );
}

/// Receipt (loop phases): user immediates and timeouts interleave with zlib
/// and fs completions exactly as in node (node 26 prints this order).
#[test]
fn internal_completions_keep_their_loop_phase() {
    // The witness schedules user timers, so TIMEOUT is rightly emitted here;
    // the internal completions themselves are covered by the cases above.
    case(
        r#"import { gzip } from "node:zlib";
import { readFile, writeFile } from "node:fs";
const log: string[] = [];
const file = process.cwd() + "/order.txt";
writeFile(file, "x", () => {
  log.push("fs-write");
  setImmediate(() => log.push("user-immediate-1"));
  gzip(Buffer.from("perry"), () => {
    log.push("zlib");
    setImmediate(() => log.push("user-immediate-2"));
    readFile(file, () => {
      log.push("fs-read");
      setImmediate(() => log.push("user-immediate-3"));
      setTimeout(() => {
        log.push("user-timeout");
        console.log(log.join(" "));
      }, 20);
    });
  });
});
"#,
        "fs-write user-immediate-1 zlib user-immediate-2 fs-read user-immediate-3 user-timeout\n",
        "TIMEOUT",
    );
}

/// Receipt (async_hooks): each internal completion is node's provider type
/// (node 26 prints exactly this). The `resource` handed to `init` is the boxed
/// timer id, not node's handle object.
#[test]
fn internal_completions_are_their_async_hooks_providers() {
    case(
        r#"import { createHook } from "node:async_hooks";
import { gzip } from "node:zlib";
import { writeFile } from "node:fs";
import { writeFile as writeFileP } from "node:fs/promises";
const types: string[] = [];
const ignore = new Set(["PROMISE", "TickObject"]);
const hook = createHook({ init(_id: number, type: string) { if (!ignore.has(type)) types.push(type); } });
hook.enable();
function at(label: string, op: () => unknown) {
  const before = types.length;
  op();
  return `${label}: ${[...new Set(types.slice(before))].join(",")}`;
}
const out: string[] = [];
const file = process.cwd() + "/hooks.txt";
out.push(at("zlib.gzip", () => gzip(Buffer.from("x"), () => {})));
out.push(at("fs.writeFile", () => writeFile(file, "x", () => {})));
out.push(at("fs/promises.writeFile", () => writeFileP(file + "2", "x")));
out.push(at("AbortSignal.timeout", () => AbortSignal.timeout(5)));
hook.disable();
setTimeout(() => console.log(out.join("\n")), 50);
"#,
        "zlib.gzip: ZLIB\nfs.writeFile: FSREQCALLBACK\nfs/promises.writeFile: FSREQPROMISE\nAbortSignal.timeout: Timeout\n",
        "TIMEOUT",
    );
}

#[test]
fn fetch_body_reads_reach_the_done_first_iterator_result() {
    case(
        r#"import http from "node:http";
const server = http.createServer((_req: any, res: any) => { res.write("ab"); res.end("c"); });
server.listen(0, "127.0.0.1", async () => {
  const response = await fetch(`http://127.0.0.1:${(server.address() as any).port}/`);
  const reader = response.body!.getReader();
  let text = "";
  for (;;) {
    const step = await reader.read();
    if (step.done) break;
    text += new TextDecoder().decode(step.value);
  }
  console.log(text);
  server.close();
});
"#,
        "abc\n",
        "ITERATOR_RESULT_DONE_FIRST",
    );
}

#[test]
fn http2_compat_responses_and_server_close_reach_their_layouts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, defined, _) = compile_and_run(
        dir.path(),
        r#"import * as http2 from "node:http2";
const server: any = http2.createServer((_req: any, res: any) => { res.write("x"); res.end("y"); });
server.listen(0, "127.0.0.1", () => {
  const client = http2.connect(`http://127.0.0.1:${server.address().port}`);
  const req = client.request({ ":path": "/" });
  let body = "";
  req.on("data", (c: any) => { body += c; });
  req.on("end", () => { console.log(body); client.close(); server.close(); });
  req.end();
});
"#,
    );
    assert_eq!(stdout, "xy\n");
    for layout in [
        "OUTGOING_MESSAGE_PROTOTYPE",
        "SERVER_RESPONSE_PROTOTYPE",
        "NATIVE_CTOR_OUTGOING_MESSAGE",
        "NATIVE_CTOR_SERVER_RESPONSE",
        "RECORD_F21_0",
        // The compat prototype reaches node:http's response prototype; a
        // construction through it (`new (proto.constructor)(req)`, a derived
        // `new F`) births the instance layout.
        "SERVER_RESPONSE",
    ] {
        assert!(
            defined.contains(layout),
            "{layout} is not emitted: {defined:?}"
        );
    }
}

/// node:tls alone (no net, no http): a pre-aborted connect fails through the
/// deferred TLS failure and a refused one through the socket's error event.
/// Both birth the socket error record.
#[test]
fn tls_connect_failures_reach_the_socket_error_record() {
    case(
        r#"import * as tls from "node:tls";
const aborted = tls.connect({ host: "127.0.0.1", port: 1, signal: AbortSignal.abort() } as any);
aborted.on("error", (e: any) => {
  console.log(e.name, e.code);
  const refused = tls.connect({ host: "127.0.0.1", port: 1 });
  refused.on("error", (e: any) => console.log(e.code));
});
"#,
        "AbortError ABORT_ERR\nECONNREFUSED\n",
        "RECORD_F21_0",
    );
}

#[test]
fn get_builtin_module_through_global_this_reaches_the_module_it_loads() {
    case(
        r#"const stream = (globalThis as any).process.getBuiltinModule("node:stream");
console.log(typeof stream.Stream.prototype.pipe);
"#,
        "function\n",
        "STREAM_PROTOTYPE",
    );
}

#[test]
fn create_require_reaches_the_module_it_loads() {
    case(
        r#"import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const path = req("path");
console.log(path.posix.parse("/a/b.txt").name);
"#,
        "b\n",
        "RECORD_F418_0",
    );
}

#[test]
fn get_builtin_module_reaches_the_module_it_loads() {
    case(
        r#"const stream = process.getBuiltinModule("stream");
console.log(typeof stream.Readable);
"#,
        "function\n",
        "STREAM_PROTOTYPE",
    );
}

#[test]
fn http_server_close_reaches_the_socket_error_record() {
    case(
        r#"import http from "node:http";
const server = http.createServer((req: any, res: any) => { res.write("x"); });
server.listen(0, () => {
  http.get({ port: (server.address() as any).port }, (res: any) => {
    res.on("data", () => { server.close(); (server as any).closeAllConnections(); });
    res.on("error", () => {});
    res.on("close", () => console.log("closed"));
  }).on("error", () => console.log("closed"));
});
"#,
        "closed\n",
        "RECORD_F21_0",
    );
}

/// The probes: a plain function's first `.prototype` read asks whether it is
/// `events.EventEmitter`, and `process.stdin` asks whether an object is on a
/// node instance layout. Neither reaches events or http, so neither layout is
/// emitted, and neither question names its descriptor.
#[test]
fn probes_do_not_need_the_layouts_they_ask_about() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, defined, _) = compile_and_run(
        dir.path(),
        r#"function F(this: any) { this.x = 1; }
console.log(typeof F.prototype, new (F as any)().x);
"#,
    );
    assert_eq!(stdout, "object 1\n");
    for absent in [
        "EVENT_EMITTER_PROTOTYPE",
        "EE_ASYNC_RESOURCE_PROTO",
        "SERVER_RESPONSE",
    ] {
        assert!(
            !defined.contains(absent),
            "{absent} emitted for a plain function: {defined:?}"
        );
    }
}

/// A program that reaches none of these builtins defines none of the layouts
/// the cases above add to R.
#[test]
fn a_program_without_the_builtins_emits_none_of_their_layouts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, defined, _) = compile_and_run(dir.path(), "console.log(1 + 1);\n");
    assert_eq!(stdout, "2\n");
    for absent in [
        "IMMEDIATE",
        "TIMEOUT",
        "ITERATOR_RESULT_DONE_FIRST",
        "OUTGOING_MESSAGE_PROTOTYPE",
        "RECORD_F21_0",
        "RECORD_F418_0",
        "STREAM_PROTOTYPE",
        "EVENT_EMITTER_PROTOTYPE",
        "EVENT_EMITTER",
        "SERVER_RESPONSE",
    ] {
        assert!(
            !defined.contains(absent),
            "{absent} emitted for hello: {defined:?}"
        );
    }
}

/// Receipt (async_hooks, deferred emits): the callback of each internal
/// child_process / cluster / IPC / MessagePort / perf_hooks / node:test
/// completion runs as the resource type node 26.12.1 reports for that
/// callback (`executionAsyncId()` mapped through `init`). Node delivers some
/// of these through a chain of resources (TickObject <- PIPEWRAP); perry
/// delivers one callback whose type is the chain's innermost resource.
#[test]
fn deferred_emits_run_as_nodes_async_hooks_providers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, _, _) = compile_and_run(
        dir.path(),
        r#"import { createHook, executionAsyncId } from "node:async_hooks";
import { exec, spawn } from "node:child_process";
import cluster from "node:cluster";
import { MessageChannel } from "node:worker_threads";
import { PerformanceObserver, performance } from "node:perf_hooks";
import { test } from "node:test";
const types = new Map<number, string>();
createHook({ init(id: number, type: string) { types.set(id, type); } }).enable();
const lines: string[] = [];
function who(label: string) {
  lines.push(`${label}: ${types.get(executionAsyncId())}`);
}
if (cluster.isPrimary) {
  cluster.on("setup", () => who("cluster setup event"));
  const w = cluster.fork();
  w.on("message", (m: string) => {
    if (m.startsWith("worker")) lines.push(m);
    if (m === "done") cluster.disconnect(() => who("cluster.disconnect callback"));
  });
  exec("true", () => who("exec callback"));
  const c = spawn("cat");
  c.stdin.write("x", () => { who("stdin write callback"); c.stdin.end(); });
  spawn("/nonexistent-perry-hooks-probe").on("error", () => who("spawn error event"));
  const { port1, port2 } = new MessageChannel();
  port2.on("message", () => { who("MessagePort message"); port1.close(); });
  port1.postMessage(1);
  const o = new PerformanceObserver(() => { who("PerformanceObserver callback"); o.disconnect(); });
  o.observe({ entryTypes: ["mark"] });
  performance.mark("m");
  test("t", () => who("node:test body"));
  process.on("exit", () => console.log(lines.sort().map((l) => "hooks| " + l).join("\n")));
} else {
  process.send!("x", () => {
    process.send!(`worker process.send callback: ${types.get(executionAsyncId())}`, () => process.send!("done"));
  });
}
"#,
    );
    let hooks: String = stdout
        .lines()
        .filter(|line| line.starts_with("hooks| "))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(hooks, "hooks| MessagePort message: MESSAGEPORT\nhooks| PerformanceObserver callback: Immediate\nhooks| cluster setup event: TickObject\nhooks| cluster.disconnect callback: PROCESSWRAP\nhooks| exec callback: PIPEWRAP\nhooks| node:test body: Test\nhooks| spawn error event: TickObject\nhooks| stdin write callback: TickObject\nhooks| worker process.send callback: TickObject\n", "stdout:\n{stdout}");
}

/// node:child_process alone (no net, no stream import): a spawned child's
/// stdio pipes are `net.Socket`s, so spawning installs node:net and births
/// its socket prototype chain down to `Stream.prototype`.
#[test]
fn child_process_pipe_sockets_reach_the_stream_layouts() {
    case(
        r#"import { spawn } from "node:child_process";
const c = spawn("sh", ["-c", "echo hi"]);
c.stdout.on("data", (d: any) => process.stdout.write("data " + String(d)));
c.on("close", (code: number) => console.log("close", code));
"#,
        "data hi\nclose 0\n",
        "STREAM_PROTOTYPE",
    );
}

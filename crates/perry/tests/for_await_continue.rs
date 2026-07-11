//! Regression tests: `continue` inside `for await (… of …)` loops.
//!
//! Every iterator-driver desugar that modeled the loop as
//! `while (!__result.done) { <body>; __result = await next() }` put the
//! advance at the body TAIL, so a `continue` in the user body jumped to the
//! `while` condition, skipped the advance, and re-processed the SAME result
//! forever — an infinite spin observed as a hang. Six copies of the shape
//! existed (`lower/stmt_loops.rs` ×3, `lower_decl/body_stmt.rs` ×2,
//! `lower_decl/body_stmt/for_await.rs`); all now advance at the TOP:
//! `while (true) { __result = await next(); if (__result.done) break; … }`.
//! (The array/lazy sync path already used `Stmt::For`'s update clause and
//! documented this exact footgun.)
//!
//! Canonical real-world failure: an SSE consumer's
//! `for await (const ev of stream) { if (ev.event === "ping") continue; … }`
//! — a large esbuild-bundled CLI app hung on the first real server ping
//! event (local mocks that never sent pings masked it).

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile + run with a hang guard: pre-fix these programs spun forever, so
/// enforce a wall-clock bound rather than letting the test suite wedge.
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

    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = child.wait_with_output().expect("collect output");
                assert!(
                    status.success(),
                    "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
                    status,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                return String::from_utf8_lossy(&out.stdout).into_owned();
            }
            None if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                panic!(
                    "compiled binary hung (pre-fix: `continue` spun on the same iterator result)"
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The SSE-ping shape: `continue` inside `for await` inside an async
/// generator must re-pull the source iterator.
#[test]
fn continue_in_for_await_inside_async_generator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function* events(): any {
  yield { event: "message_start", data: '{"a":1}' };
  yield { event: "ping", data: "{}" };
  yield { event: "content_block_delta", data: '{"b":2}' };
  yield { event: "ping", data: "{}" };
}
async function* filtered(): any {
  for await (const w of events()) {
    if (w.event === "ping") continue;
    yield JSON.parse(w.data);
  }
}
(async () => {
  for await (const x of filtered()) console.log("got", JSON.stringify(x));
  console.log("done");
})();
"#,
    );
    assert_eq!(stdout, "got {\"a\":1}\ngot {\"b\":2}\ndone\n");
}

/// `continue` inside `for await` in a plain async function.
#[test]
fn continue_in_for_await_inside_async_fn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function* nums(): any { yield 1; yield 2; yield 3; yield 4; }
(async () => {
  const out: number[] = [];
  for await (const n of nums()) {
    if (n % 2 === 0) continue;
    out.push(n);
  }
  console.log(JSON.stringify(out));
})();
"#,
    );
    assert_eq!(stdout, "[1,3]\n");
}

/// `continue` over a custom `[Symbol.asyncIterator]` object (the runtime
/// GetAsyncIterator driver path, not the recognized-generator-call path).
#[test]
fn continue_in_for_await_over_custom_async_iterable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function* mk(): any { yield "a"; yield "skip"; yield "b"; }
const src: any = { [Symbol.asyncIterator]() { return mk(); } };
(async () => {
  const out: string[] = [];
  for await (const s of src) {
    if (s === "skip") continue;
    out.push(s);
  }
  console.log(JSON.stringify(out));
})();
"#,
    );
    assert_eq!(stdout, "[\"a\",\"b\"]\n");
}

/// `continue` over a Web ReadableStream (the getReader()/read() driver).
#[test]
fn continue_in_for_await_over_readable_stream() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const rs = new ReadableStream({
  start(c: any) {
    c.enqueue("x");
    c.enqueue("skip");
    c.enqueue("y");
    c.close();
  },
});
(async () => {
  const out: string[] = [];
  for await (const chunk of rs as any) {
    if (chunk === "skip") continue;
    out.push(chunk);
  }
  console.log(JSON.stringify(out));
})();
"#,
    );
    assert_eq!(stdout, "[\"x\",\"y\"]\n");
}

/// Loop-exit semantics stay intact after the restructure: `break` exits the
/// driver, and a NESTED loop's own `continue` doesn't touch the outer
/// driver's advance. (Whether `break` also runs IteratorClose — the source
/// generator's `finally` — is a separate, pre-existing gap on the
/// recognized-generator-call path: the abrupt-close rewrite is only applied
/// for node-stream-like sources. Unchanged by this fix, so not asserted.)
#[test]
fn break_and_nested_continue_still_behave() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function* src(): any { yield 1; yield 2; yield 3; yield 4; }
(async () => {
  const out: number[] = [];
  for await (const n of src()) {
    for (const k of [10, 20]) {
      if (k === 10) continue; // inner continue must not touch the outer driver
      out.push(n * k);
    }
    if (n === 3) break;
  }
  console.log(JSON.stringify(out));
})();
"#,
    );
    assert_eq!(stdout, "[20,40,60]\n");
}

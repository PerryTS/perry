//! Regression tests for the #4909 client-side OutgoingMessage + timeout
//! sub-tickets: `req.write(chunk, cb)` / `req.end(cb)` fire their callbacks
//! in Node's flush order (write callbacks → `'finish'` → end callback),
//! `req.write()` returns a real backpressure boolean, and
//! `req.setTimeout(ms, cb)` emits `'timeout'` even when the server never
//! responds — followed by the coded ECONNRESET + `'close'` teardown when the
//! handler destroys the request.
//!
//! Pre-fix, the static dispatch table routed `req.write`/`req.end` to
//! single-arg entry points that dropped the callbacks and returned the
//! (always-truthy) handle from `write()`, so `while (req.write(buf, cb))`
//! producer loops spun forever; `req.setTimeout()` only stored the delay,
//! so a never-responding server hung the process — the dominant silent-hang
//! mode in the #4909 corpus bucket.

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

/// Asserts `needles` appear in `haystack` in the given order.
fn assert_ordered(haystack: &str, needles: &[&str]) {
    let mut from = 0;
    for needle in needles {
        match haystack[from..].find(needle) {
            Some(pos) => from += pos + needle.len(),
            None => panic!("expected \"{needle}\" (in order) in output:\n{haystack}"),
        }
    }
}

/// Client + server write/end callbacks, backpressure boolean, and Node's
/// flush ordering (write cbs → 'finish' → end cb) on both OutgoingMessage
/// directions.
#[test]
fn write_end_callbacks_fire_in_flush_order_with_backpressure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const http = require('http');
const server = http.createServer((req: any, res: any) => {
  req.resume();
  req.on('end', () => {
    res.write('x', () => console.log('res-wcb'));
    res.on('finish', () => console.log('res-finish'));
    res.end('y', () => console.log('res-endcb'));
  });
  server.close();
}).listen(0, function () {
  const req = http.request({ port: server.address().port, method: 'PUT' });
  const big = Buffer.alloc(16 * 1024, 'x');
  // First 16 KiB write sits exactly at the high-water mark (true), the
  // second one passes it (false) — the Node backpressure contract that
  // terminates `while (req.write(buf))` producer loops.
  console.log('w1', req.write(big, () => console.log('req-wcb1')));
  console.log('w2', req.write(big, () => console.log('req-wcb2')));
  req.on('finish', () => console.log('req-finish'));
  req.end(() => console.log('req-endcb'));
  req.on('response', (res: any) => { console.log('status', res.statusCode); });
});
"#,
    );

    assert_ordered(
        &stdout,
        &[
            "w1 true",
            "w2 false",
            "req-wcb1",
            "req-wcb2",
            "req-finish",
            "req-endcb",
            "res-wcb",
            "res-finish",
            "res-endcb",
            "status 200",
        ],
    );
}

/// `req.setTimeout(ms, cb)` against a server that never responds: the
/// `'timeout'` event must fire (pre-fix the delay was stored but no timer
/// existed, hanging forever), and the canonical destroy-in-handler pattern
/// gets the coded ECONNRESET "socket hang up" then `'close'` with
/// `req.destroyed === true`.
#[test]
fn set_timeout_fires_timeout_event_and_destroy_tears_down() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const http = require('http');
const server = http.createServer(() => { /* never respond */ });
server.listen(0, function () {
  const req = http.request({ port: server.address().port });
  req.setTimeout(50, () => {
    console.log('timeout-fired');
    req.destroy();
  });
  req.on('error', (e: any) => console.log('error-code', e.code));
  req.on('close', () => {
    console.log('close-destroyed', req.destroyed);
    server.close();
  });
  req.end();
});
"#,
    );

    assert_ordered(
        &stdout,
        &[
            "timeout-fired",
            "error-code ECONNRESET",
            "close-destroyed true",
        ],
    );
}

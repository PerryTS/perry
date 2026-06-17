//! Regression test for #5174: instantiating `globalThis.Headers` inside an
//! in-process `node:http` server handler wedged the HTTP response pump — the
//! client's response callback never fired and the process hung (exit 124 under
//! `timeout`).
//!
//! Root cause: `new Headers()` flips the compiler's `uses_fetch` flag, which
//! used to keep the whole `http-client` perry-stdlib feature in the
//! auto-optimize rebuild. That feature compiles BOTH the Web Fetch API
//! (`src/fetch/`) AND the bundled node:http client (`src/http.rs` /
//! `src/axios.rs`). With `node:http` simultaneously routed to `perry-ext-http`,
//! both crates exported `js_http_process_pending` (et al.); perry-ext-http's
//! aux-pump shim bound to perry-stdlib's bundled copy, which drains a different
//! (always-empty) queue. The real response events piled up in perry-ext-http's
//! queue forever.
//!
//! Fix: split `web-fetch` (the Web Fetch FFIs) out of `http-client` so the
//! well-known flip can strip *only* the bundled node:http client while keeping
//! the Web Fetch data types a bare `new Headers()` needs.
//!
//! Needs the default (auto-optimize) build — `PERRY_NO_AUTO_OPTIMIZE=1`
//! produces a runtime-only binary where `node:http` dispatch is a no-op stub.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &std::path::Path, source: &str) -> PathBuf {
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

    // The bundled node:http client previously collided with perry-ext-http;
    // the fix should leave the link clean.
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(
        !stderr.contains("duplicate symbol '_js_http_process_pending'")
            && !stderr.contains("duplicate symbol '_js_http_status_message'"),
        "fix should not link both the bundled node:http client and \
         perry-ext-http (duplicate js_http_* symbols):\n{stderr}"
    );

    output
}

/// Run `bin`, failing if it doesn't exit on its own within `secs` — the #5174
/// signature is a hang (the in-process client callback never fires), so a
/// wall-clock guard is the regression's real assertion. A reader thread drains
/// stdout (so a full pipe buffer can't itself stall the child) while the main
/// thread polls for exit against the deadline.
fn run_with_timeout(bin: &std::path::Path, secs: u64) -> String {
    use std::io::Read;

    let mut child = Command::new(bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn compiled binary");

    let mut piped = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = piped.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let stdout = reader.join().unwrap_or_default();
                assert!(
                    status.success(),
                    "binary exited non-zero: {status:?}\nstdout:\n{stdout}"
                );
                return stdout;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = reader.join().unwrap_or_default();
                panic!(
                    "#5174 regression: process hung for >{secs}s — the \
                     in-process HTTP response pump never delivered the response \
                     (Headers + node:http server/client wedge).\nstdout so far:\n{stdout}"
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The issue's minimal repro: `new globalThis.Headers(...)` in the request
/// handler must not wedge the in-process client's response callback.
#[test]
fn headers_in_http_handler_does_not_hang_response_pump() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
import http from "node:http";
const server = http.createServer((req, res) => {
  // Constructed, never used — this is the line that wedged the pump (#5174).
  const headers = new globalThis.Headers({ foo: "1" });
  void headers;
  res.writeHead(200, { foo: "1", bar: "2" });
  res.end();
});
server.listen(0, () => {
  const port = (server.address() as { port: number }).port;
  const req = http.get({ port }, (res) => {
    console.log("STATUS", res.statusCode);
    res.on("end", () => server.close());
    res.resume();
  });
  req.on("error", (e) => console.error("client err", e.message));
});
"#,
    );
    let stdout = run_with_timeout(&bin, 30);
    assert_eq!(stdout, "STATUS 200\n");
}

/// Web Fetch and node:http coexisting end-to-end: a built-in `fetch()` against
/// the in-process server, with `Headers` *methods* (`append`/`get`) exercised
/// in the handler. Confirms the Web Fetch half still works once split from the
/// bundled node:http client.
#[test]
fn fetch_against_in_process_http_server_with_headers_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
import http from "node:http";
const server = http.createServer((req, res) => {
  const h = new globalThis.Headers({ foo: "1" });
  h.append("foo", "2");
  res.writeHead(200, { "x-test": h.get("foo")! });
  res.end("hello");
});
server.listen(0, async () => {
  const port = (server.address() as { port: number }).port;
  const r = await fetch(`http://127.0.0.1:${port}/`);
  const body = await r.text();
  console.log("FETCH", r.status, r.headers.get("x-test"), body);
  server.close();
});
"#,
    );
    let stdout = run_with_timeout(&bin, 30);
    assert_eq!(stdout, "FETCH 200 1, 2 hello\n");
}

//! Regression (#5783): `IncomingMessage.pipe(dest)` must (a) return `dest`
//! and (b) forward the response body to `dest.write()` / `dest.end()`.
//!
//! Before the fix, a client response's `.pipe()` was unimplemented: it returned
//! `undefined` and never moved any bytes into the destination stream. The
//! canonical victim is node-fetch, which reads every response body as
//! `const body = res.pipe(new PassThrough())` and then consumes `body`. With a
//! dead PassThrough, `response.text()` never settled, so the gaxios/node-fetch
//! request promise hung forever — which stalled the TUI onboarding's preflight
//! connectivity check (its `await client.get(...)` never returned), so the app
//! painted the splash and then hung awaiting a step that could never advance.
//!
//! This drives the exact shape against a local HTTP server (deterministic, no
//! external network): `http.get(...).pipe(new PassThrough())`, read the
//! PassThrough, and assert the full body arrives and the process exits.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, source: &str) -> String {
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

    // Run under a wall-clock timeout: this is a regression test for a HANG, so
    // a plain `output()` would stall until the job-level timeout if the bug
    // returns. Poll `try_wait`, kill + fail fast otherwise.
    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");
    let timeout = Duration::from_secs(30);
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait on compiled binary") {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "compiled binary did not exit within {timeout:?} — \
                 IncomingMessage.pipe() likely regressed to a no-op/hang"
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_string(&mut stdout).ok();
    }
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        err.read_to_string(&mut stderr).ok();
    }
    assert!(
        status.success(),
        "compiled binary failed\nstatus: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn incoming_message_pipe_forwards_body_and_returns_dest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
import http from "http";
import { PassThrough } from "stream";

const BODY = '{"ok":true,"x":12345}';
const srv = http.createServer((req: any, res: any): void => {
  res.writeHead(200, { "Content-Type": "application/json", "Content-Length": String(BODY.length) });
  res.end(BODY);
});
srv.listen(0, (): void => {
  const port = (srv.address() as any).port;
  http.get(`http://127.0.0.1:${port}/`, (res: any): void => {
    // node-fetch's exact pattern: pipe the response into a PassThrough and
    // keep only the return value as the readable body.
    const body = res.pipe(new PassThrough());
    if (!body) {
      console.log("RET:undefined");
      srv.close();
      return;
    }
    console.log("RET:dest");
    const chunks: any[] = [];
    body.on("data", (c: any): void => { chunks.push(c); });
    body.on("end", (): void => {
      console.log("BODY:" + Buffer.concat(chunks).toString());
      srv.close();
    });
  });
});
"#,
    );

    assert!(
        out.contains("RET:dest"),
        "pipe() must return the destination stream, not undefined: {out}"
    );
    assert!(
        out.contains(r#"BODY:{"ok":true,"x":12345}"#),
        "piped PassThrough never received the full response body: {out}"
    );
}

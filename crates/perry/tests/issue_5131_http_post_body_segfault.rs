//! Regression test for #5131: a `node:http` server that consumes the request
//! body (`req.on('data')` + `req.on('end')`) on a request that actually carries
//! a body segfaulted (SIGSEGV / exit 139).
//!
//! The crash was fixed on `main` by the http/stream work that landed after the
//! issue was filed (v0.5.1167); this test guards against a regression. It needs
//! the default (auto-optimize) build — `PERRY_NO_AUTO_OPTIMIZE=1` produces a
//! runtime-only binary where `node:http` dispatch is a no-op stub.

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
        "compiled binary failed (pre-fix: SIGSEGV / exit 139 on body consume)\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The issue's repro: a POST whose body is consumed via `req.on('data')` /
/// `req.on('end')` must round-trip without crashing.
#[test]
fn http_server_consuming_post_body_does_not_segfault() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import http from "node:http";
const server = http.createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => { res.writeHead(200); res.end("len:" + body.length); });
});
server.listen(0, () => {
  const port = (server.address() as { port: number }).port;
  const r = http.request({ port, method: "POST" }, (res) => {
    let d = ""; res.on("data", (c) => (d += c));
    res.on("end", () => { console.log(d); server.close(); });
  });
  r.write("payload-bytes");
  r.end();
});
"#,
    );
    assert_eq!(stdout, "len:13\n");
}

/// A larger, chunk-collected body (`Buffer.concat`) must also round-trip — this
/// exercises the multi-chunk data path and more allocation pressure.
#[test]
fn http_server_consuming_large_post_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import http from "node:http";
const big = "x".repeat(100000);
const server = http.createServer((req, res) => {
  const chunks: Buffer[] = [];
  req.on("data", (c) => chunks.push(c as Buffer));
  req.on("end", () => {
    const body = Buffer.concat(chunks).toString();
    res.writeHead(200);
    res.end("len:" + body.length);
  });
});
server.listen(0, () => {
  const port = (server.address() as { port: number }).port;
  const r = http.request({ port, method: "POST" }, (res) => {
    let d = ""; res.on("data", (c) => (d += c));
    res.on("end", () => { console.log(d); server.close(); });
  });
  r.write(big);
  r.end();
});
"#,
    );
    assert_eq!(stdout, "len:100000\n");
}

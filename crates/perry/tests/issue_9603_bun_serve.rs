//! #9603 — Bun.serve facade backed by Perry's native HTTP server.

use std::io::Read;
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
    output
}

fn run_with_timeout(bin: &std::path::Path, secs: u64) -> (String, String) {
    let mut child = Command::new(bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout_pipe.read_to_string(&mut output);
        output
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stderr_pipe.read_to_string(&mut output);
        output
    });

    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let stdout = stdout_reader.join().unwrap_or_default();
                let stderr = stderr_reader.join().unwrap_or_default();
                assert!(
                    status.success(),
                    "compiled Bun.serve fixture failed\nstatus: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
                return (stdout, stderr);
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = stdout_reader.join().unwrap_or_default();
                let stderr = stderr_reader.join().unwrap_or_default();
                panic!("Bun.serve fixture hung for >{secs}s\nstdout:\n{stdout}\nstderr:\n{stderr}");
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[test]
fn named_serve_handles_fetch_responses_errors_and_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
import { serve } from "bun";

const server = serve({
  hostname: "127.0.0.1",
  port: 0,
  idleTimeout: 0,
  development: false,
  async fetch(request: Request, activeServer: any) {
    const url = new URL(request.url);
    if (url.pathname === "/error") {
      throw new Error("boom");
    }
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
    const address = activeServer.requestIP(request)?.address ?? "unknown";
    return new Response(`${request.method}:${address}`, {
      status: 201,
      headers: { "x-bun": "perry" },
    });
  },
  async error(error: Error) {
    return new Response(error.message, { status: 503 });
  },
});

console.log(server.hostname, server.port > 0, server.development, server.protocol);
const response = await fetch(`http://127.0.0.1:${server.port}/`);
console.log(response.status, response.headers.get("x-bun"), await response.text());
const failed = await fetch(`http://127.0.0.1:${server.port}/error`);
console.log(failed.status, await failed.text());
server.unref();
server.ref();
await server.stop(true);
console.log("stopped");
"#,
    );
    let (stdout, stderr) = run_with_timeout(&bin, 30);
    assert_eq!(
        stdout, "127.0.0.1 true false http\n201 perry GET:127.0.0.1\n503 boom\nstopped\n",
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn tls_options_fail_with_an_explicit_diagnostic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
import { serve } from "bun";
try {
  serve({ port: 0, tls: { key: "x", cert: "x" }, fetch: () => new Response("x") });
} catch (error: any) {
  console.log(error.code, error.message);
}
"#,
    );
    let (stdout, stderr) = run_with_timeout(&bin, 30);
    assert_eq!(
        stdout, "ERR_NOT_SUPPORTED Bun.serve TLS options are not supported by Perry yet\n",
        "unexpected stderr:\n{stderr}"
    );
}

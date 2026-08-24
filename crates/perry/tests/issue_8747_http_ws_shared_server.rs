//! Regression test for #8747: attaching `ws.WebSocketServer` to a
//! `node:http` server made the first ordinary HTTP request abort in
//! `TcpListener::from_std` with "there is no reactor running".
//!
//! The `ws` import changes the native-wrapper link shape. The HTTP accept loop
//! must therefore be scheduled through Perry's explicit shared-runtime bridge,
//! rather than relying on an ambient Tokio context in an FFI callback.

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
    use std::io::Read;

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
                    "#8747 regression: shared HTTP/WS server aborted\n\
                     status: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
                return (stdout, stderr);
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = stdout_reader.join().unwrap_or_default();
                let stderr = stderr_reader.join().unwrap_or_default();
                panic!(
                    "#8747 regression: shared HTTP/WS server hung for >{secs}s\n\
                     stdout:\n{stdout}\nstderr:\n{stderr}"
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[test]
fn plain_http_request_survives_attached_websocket_server() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
import { createServer } from "node:http";
import { WebSocketServer } from "ws";

const httpServer = createServer((_req, res) => {
  res.writeHead(200);
  res.end("http-ok");
});
const wss = new WebSocketServer({ server: httpServer });

await new Promise<void>((resolve) => httpServer.listen(0, resolve));
const port = (httpServer.address() as { port: number }).port;
const response = await fetch("http://127.0.0.1:" + port + "/");
console.log(response.status, await response.text());

wss.close();
httpServer.close();
process.exit(0);
"#,
    );
    let (stdout, stderr) = run_with_timeout(&bin, 30);
    assert_eq!(stdout, "200 http-ok\n", "unexpected stderr:\n{stderr}");
}

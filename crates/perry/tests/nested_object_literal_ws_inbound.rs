//! Regression: a deeply nested object/array literal at module scope must not
//! corrupt an adjacent native handle. On v0.5.1206 a literal of the shape
//! `{ a: [..], b: [{ kind, child: { a: [..], b: [{ kind }] } }] }` — an object
//! whose array element holds a nested object that itself holds an array of objects
//! — silently broke `node:http` WebSocket *inbound* dispatch GLOBALLY: the upgrade
//! handler's `wsId.on("message", …)` never fired, so an echo server stopped echoing
//! while outbound `wsId.send(...)` still worked. The literal itself read back
//! correctly — the corruption hit a neighbouring allocation (the WS client handle),
//! so the only observable was the dropped inbound frame. A flat, single-level
//! literal was unaffected. Fixed after v0.5.1206; this test guards the regression.
//!
//! The fixture self-tests in-process: a WS echo server + a `ws` client that sends
//! "ping" and expects "echo:ping". A miscompile -> no echo -> hang (the Rust
//! timeout, or the fixture's own 8s `exit(1)`); a healthy build -> prints `WSOK`
//! and exits 0.
//!
//! Needs the default (auto-optimize) build for the node:http/ws server symbols.

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

/// Run `bin`, failing if it doesn't exit on its own within `secs` — the buggy
/// signature is a hang (inbound frame never dispatched). A reader thread drains
/// stdout while the main thread polls for exit against the deadline.
fn run_with_timeout(bin: &std::path::Path, secs: u64) -> String {
    use std::io::Read;

    let mut child = Command::new(bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");

    let mut piped = child.stdout.take().expect("piped stdout");
    let mut err_piped = child.stderr.take().expect("piped stderr");
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = piped.read_to_string(&mut buf);
        buf
    });
    // Capture stderr too — the fixture writes its own failure signal there
    // (`console.error("no echo …")`), which is the most useful breadcrumb when CI fails.
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = err_piped.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let stdout = reader.join().unwrap_or_default();
                let stderr = err_reader.join().unwrap_or_default();
                assert!(
                    status.success(),
                    "binary exited non-zero: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
                return stdout;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = reader.join().unwrap_or_default();
                let stderr = err_reader.join().unwrap_or_default();
                panic!(
                    "nested-literal regression: process hung for >{secs}s — the WS \
                     upgrade handler's inbound `message` never fired (a nested object \
                     literal corrupted the adjacent WS handle).\nstdout so far:\n{stdout}\nstderr so far:\n{stderr}"
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[test]
fn nested_object_literal_does_not_break_ws_inbound() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(
        dir.path(),
        r#"
import { createServer } from "node:http";
import WebSocket from "ws";

// The nested literal that regressed. Defined at module scope and merely read —
// the corruption hit a neighbouring native allocation, not the literal itself.
const X: any = {
  a: ["x", "y"],
  b: [{ kind: "outer", child: { a: ["x", "y"], b: [{ kind: "inner" }] } }],
};

const server = createServer((req: any, res: any) => res.end("n=" + X.b.length));
server.on("upgrade", (req: any, wsId: any) => {
  wsId.on("message", (data: any) => {
    wsId.send("echo:" + String(data));
  });
});
// fail fast (rather than hang) if inbound never round-trips
const timer = setTimeout(() => {
  console.error("no echo — WS inbound dropped");
  process.exit(1);
}, 8000);
server.listen(0, () => {
  const port = (server.address() as { port: number }).port;
  const ws = new WebSocket("ws://127.0.0.1:" + port);
  ws.on("open", () => ws.send("ping"));
  ws.on("message", (d: any) => {
    if (String(d) === "echo:ping") {
      console.log("WSOK");
      clearTimeout(timer);
      process.exit(0); // deterministic — don't depend on async close draining the loop
    }
  });
});
"#,
    );
    let stdout = run_with_timeout(&bin, 40);
    assert_eq!(
        stdout, "WSOK\n",
        "expected the WS echo round-trip to complete"
    );
}

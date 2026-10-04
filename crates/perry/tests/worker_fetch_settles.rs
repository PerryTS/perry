//! A `fetch` started on a Worker thread must settle.
//!
//! A worker owns its own network loop, and only a turn of that loop on the
//! worker's thread completes the worker's I/O. While waiting for messages the
//! worker blocked on its command channel and drained microtasks and its own
//! timers, but never turned that loop, so a fetch in a worker stayed pending
//! for ever — no response, no error, not even when its AbortSignal fired.
//! OpenCode's TUI runs its server in a worker; the server's fetch of
//! models.dev timed out and the TUI shut down without painting.
//!
//! The second half checks the other direction: while the worker is parked in
//! its loop waiting for that fetch, a message from the main thread must still
//! be delivered promptly, so the `pong` arrives before the slow response.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

const WORKER_SOURCE: &str = r#"
// @ts-nocheck
onmessage = (e) => {
  if (e.data.port) {
    fetch("http://127.0.0.1:" + e.data.port + "/slow")
      .then((r) => r.text())
      .then((t) => postMessage("fetch " + t))
      .catch((err) => postMessage("fetch error " + err))
    return
  }
  postMessage(e.data + " pong")
}
"#;

const MAIN_SOURCE: &str = r#"
// @ts-nocheck
import http from "node:http"
const seen = []
const server = http.createServer((req, res) => {
  setTimeout(() => res.end("hello"), 300)
})
server.listen(0, "127.0.0.1", () => {
  const worker = new Worker("./worker.ts", { type: "module" })
  worker.onmessage = (e) => {
    seen.push(e.data)
    if (seen.length === 2) {
      console.log(seen.join("\n"))
      process.exit(0)
    }
  }
  worker.postMessage({ port: server.address().port })
  setTimeout(() => worker.postMessage("ping"), 100)
})
setTimeout(() => {
  console.log("TIMEOUT; got: " + JSON.stringify(seen))
  process.exit(1)
}, 10000)
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "ping pong\nfetch hello\n";

#[test]
fn worker_fetch_settles_and_messages_still_arrive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("worker.ts"), WORKER_SOURCE).unwrap();
    std::fs::write(root.join("main.ts"), MAIN_SOURCE).unwrap();

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(root)
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "pre-fix the worker's fetch never settled\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, EXPECTED,
        "the fetch must settle and the ping must not wait for it"
    );
}

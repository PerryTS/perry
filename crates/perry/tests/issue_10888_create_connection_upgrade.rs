//! Regression for #10888: an upgrade request that supplies `createConnection`
//! must preserve `Connection: Upgrade` and return that same live socket from
//! the request's `upgrade` event.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"
import { createServer, request } from "node:http";
import { connect } from "node:net";

const watchdog = setTimeout(() => {
  console.log("timeout");
  process.exit(1);
}, 5000);

const server = createServer((_req: any, res: any) => {
  console.log("ordinary request");
  res.statusCode = 426;
  res.end();
});
server.on("upgrade", (_req: any, socket: any) => {
  console.log("server upgrade");
  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\n" +
      "Connection: Upgrade\r\n" +
      "Upgrade: probe\r\n\r\n",
  );
  setTimeout(() => socket.write("hello"), 20);
});
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  const req = request({
    host: "127.0.0.1",
    port,
    headers: { Connection: "Upgrade", Upgrade: "probe" },
    createConnection: (options: any) => connect(options.port, options.host),
  });
  req.on("upgrade", (res: any, socket: any, head: any) => {
    console.log("client upgrade", res.statusCode, res.headers.upgrade, head.length);
    socket.on("data", (data: any) => {
      console.log("client data", data.toString());
      clearTimeout(watchdog);
      socket.destroy();
      server.close(() => process.exit(0));
    });
  });
  req.on("response", (res: any) => console.log("response", res.statusCode));
  req.on("error", (error: any) => console.log("request error", error.message));
  req.end();
});
"#;

const CAPTURED_CLASS_STATIC_SOURCE: &str = r#"
function make() {
  const marker = "captured";
  function state() {
    return SocketState.CONNECTING;
  }
  class SocketState {
    constructor() {
      this.marker = marker;
      this.state = SocketState.CONNECTING;
    }
  }
  Object.defineProperty(SocketState, "CONNECTING", { value: 0 });
  return { SocketState, state };
}

const { SocketState, state } = make();
const socket = new SocketState();
console.log(socket.marker, socket.state, state(), SocketState.CONNECTING);
"#;

#[test]
fn custom_connection_preserves_upgrade_and_hands_back_the_live_socket() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main");
    std::fs::write(&entry, SOURCE).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            binary.to_str().unwrap(),
            "--no-cache",
        ])
        .env("PERRY_WORKSPACE_ROOT", root)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut child = Command::new(binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "compiled fixture hung: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "compiled fixture failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "server upgrade\nclient upgrade 101 probe 0\nclient data hello\n"
    );
}

#[test]
fn captured_class_self_reads_runtime_static_properties() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main");
    std::fs::write(&entry, CAPTURED_CLASS_STATIC_SOURCE).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            binary.to_str().unwrap(),
            "--no-cache",
        ])
        .env("PERRY_WORKSPACE_ROOT", root)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let output = Command::new(binary).output().expect("run compiled fixture");
    assert!(
        output.status.success(),
        "compiled fixture failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "captured 0 0 0\n"
    );
}

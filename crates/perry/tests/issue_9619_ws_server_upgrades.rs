//! Exercise real HTTP and WebSocket traffic through compiled native bindings.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"
import { createServer } from "node:http";
import { WebSocketServer, WebSocket } from "ws";
const mode = "@MODE@";
const total = @TOTAL@;
let connections = 0, callbacks = 0, messages = 0, opened = 0, errors = 0;
let urls = 0, members = 0, listening = 0;
const http = createServer((req, res) => res.end("http-ok"));
const wss = new WebSocketServer(@OPTIONS@);
const clients = wss.clients;
if (mode === "manual" || mode === "callback-only") {
  let message = "";
  try { wss.address(); } catch (error) { message = error.message; }
  if (message !== 'The server is operating in "noServer" mode') throw new Error("noServer address");
}
const watchdog = setTimeout(() => {
  console.log("timeout", connections, callbacks, messages, opened, errors, listening);
  process.exit(1);
}, 10000);
function emitConnection(server: any, event: string, ws: any, req: any) {
  return server[event]("connection", ws, req);
}
function sendHello(ws: any) { ws.send("hello"); }
wss.on("connection", (ws, req) => {
  connections++;
  if (mode === "ephemeral" || req.url === "/v1/ws?token=ok") urls++;
  if (clients.has(ws)) members++;
  sendHello(ws);
});
if (mode === "manual" || mode === "callback-only") {
  http.on("upgrade", (req, socket, head) => {
    if (req.url.split("?")[0] !== "/v1/ws") return;
    const result = wss.handleUpgrade(req, socket, head, (ws, request) => {
      callbacks++;
      if (request !== req) throw new Error("request identity");
      if (mode === "manual") {
        if (!emitConnection(wss, "emit", ws, req)) throw new Error("emit return");
      } else { sendHello(ws); }
    });
    if (result !== undefined) throw new Error("handleUpgrade return");
  });
}
function connect(port: number) {
  for (let i = 0; i < total; i++) {
    const client = new WebSocket("ws://127.0.0.1:" + port + "/v1/ws?token=ok");
    client.on("open", () => { opened++; });
    client.on("error", () => { errors++; });
    client.on("message", (data) => {
      if (data.toString() !== "hello") throw new Error("message payload");
      messages++;
      client.close();
      if (messages === total) {
        setTimeout(async () => {
          console.log("counts", connections, callbacks, messages, opened, errors, urls, members, listening);
          wss.close();
          if (mode === "attached") {
            const response = await fetch("http://127.0.0.1:" + port + "/after-ws-close");
            console.log("detached-http", await response.text());
          }
          if (mode !== "ephemeral") http.close();
          clearTimeout(watchdog);
        }, 30);
      }
    });
  }
}
wss.on("listening", () => {
  listening++;
  const address = wss.address();
  console.log("address", address.address === "127.0.0.1", address.family === "IPv4", address.port > 0);
  if (mode === "ephemeral") connect(address.port);
});
if (mode !== "ephemeral") {
  http.listen(0, "127.0.0.1", async () => {
    const port = http.address().port;
    console.log("http", await (await fetch("http://127.0.0.1:" + port + "/")).text());
    if (mode === "attached") console.log("shared-port", wss.address().port === port);
    connect(port);
  });
}
"#;

fn run(mode: &str, options: &str, total: usize) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = SOURCE
        .replace("@MODE@", mode)
        .replace("@OPTIONS@", options)
        .replace("@TOTAL@", &total.to_string());
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main");
    std::fs::write(&entry, source).unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "{mode} hung: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{mode} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn attached_server_shares_http_port_and_accepts_120_clients() {
    let output = run("attached", "{ clientTracking: true, server: http }", 120);
    assert!(output.contains("http http-ok\n"), "{output}");
    assert!(output.contains("shared-port true\n"), "{output}");
    assert!(output.contains("detached-http http-ok\n"), "{output}");
    assert!(output.contains("address true true true\n"), "{output}");
    assert!(
        output.contains("counts 120 0 120 120 0 120 120 1\n"),
        "{output}"
    );
}

#[test]
fn manual_upgrade_calls_callback_and_emits_exactly_once_for_60_clients() {
    let output = run("manual", "{ maxPayload: 1024, noServer: true }", 60);
    assert!(output.contains("http http-ok\n"), "{output}");
    assert!(
        output.contains("counts 60 60 60 60 0 60 60 0\n"),
        "{output}"
    );
}

#[test]
fn handle_upgrade_does_not_emit_connection_without_callback_emission() {
    let output = run("callback-only", "{ noServer: true }", 3);
    assert!(output.contains("counts 0 3 3 3 0 0 0 0\n"), "{output}");
}

#[test]
fn ephemeral_port_listens_and_address_locates_the_server() {
    let output = run(
        "ephemeral",
        "{ clientTracking: true, host: '127.0.0.1', port: 0 }",
        6,
    );
    assert!(output.contains("address true true true\n"), "{output}");
    assert!(output.contains("counts 6 0 6 6 0 6 6 1\n"), "{output}");
}

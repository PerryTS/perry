//! #9605 — Bun.listen/Bun.connect TCP and Unix socket compatibility.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUN_TIMEOUT: Duration = Duration::from_secs(30);

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("--no-cache")
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

    let mut child = Command::new(&output)
        .current_dir(dir.path())
        .env("PERRY_GC_HEAP_LIMIT", "8")
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run compiled binary");
    let deadline = Instant::now() + RUN_TIMEOUT;
    loop {
        if child.try_wait().expect("poll compiled binary").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("reap timed-out binary");
            panic!(
                "compiled binary timed out after {RUN_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let run = child.wait_with_output().expect("collect binary output");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn named_exports_exchange_data_and_expose_socket_metadata() {
    let output = compile_and_run(
        r#"
import { listen, connect } from "bun";

const start = listen;
const dial = connect;

let received = "";
let reply = "";
let serverWrite = -1;
let clientWrite = -1;
let serverMetadata = false;
let clientMetadata = false;
let clientState = "";
let clientOrder = "";
let repeatEnd = 0;
let finish: any;
const done = new Promise<void>((resolve) => { finish = resolve; });

const server = start({
  hostname: "127.0.0.1",
  port: 0,
  data: { tag: "initial" },
  socket: {
    open(socket: any) {
      serverMetadata = socket.localAddress === "127.0.0.1" &&
        socket.localPort === server.port &&
        socket.remoteAddress === "127.0.0.1" &&
        socket.remotePort > 0 &&
        socket.localFamily === "IPv4" &&
        socket.remoteFamily === "IPv4" &&
        socket.listener === server;
      socket.data = { tag: "accepted" };
    },
    data(socket: any, bytes: Buffer) {
      received += bytes.toString();
      serverWrite = socket.write("pong");
      socket.end();
    },
    drain(_socket: any) {},
    close(_socket: any, _error: Error | undefined) {},
    error(_socket: any, error: Error) { throw error; },
  },
});
server.data = { tag: "listener" };

const client = await dial({
  hostname: "127.0.0.1",
  port: server.port,
  data: { writes: 0 },
  socket: {
    open(socket: any) {
      clientOrder += "open,";
      clientMetadata = socket.localAddress === "127.0.0.1" &&
        socket.localPort > 0 &&
        socket.remoteAddress === "127.0.0.1" &&
        socket.remotePort === server.port &&
        socket.localFamily === "IPv4" &&
        socket.remoteFamily === "IPv4";
      socket.data.writes++;
      clientWrite = socket.write("ping");
      socket.end();
      repeatEnd = socket.end();
    },
    data(socket: any, bytes: Buffer) {
      clientOrder += "data,";
      reply += bytes.toString();
      clientState = String(socket.data.writes);
    },
    drain(_socket: any) {},
    end(_socket: any) { clientOrder += "end,"; },
    close(_socket: any, error: Error | undefined) {
      clientOrder += "close";
      if (error) throw error;
      finish();
    },
    error(_socket: any, error: Error) { throw error; },
  },
});

await done;
console.log(server.port > 0, server.hostname, server.data.tag);
console.log(received, reply, serverWrite, clientWrite, clientState);
console.log(serverMetadata, clientMetadata, clientOrder, repeatEnd);
console.log(client.end(), client.data.writes, client.readyState);
server.stop(true);
"#,
    );
    assert_eq!(
        output,
        "true 127.0.0.1 listener\nping pong 4 4 1\ntrue true open,data,end,close -1\n-1 1 0\n"
    );
}

#[test]
fn large_writes_report_partial_admission_and_resume_on_drain() {
    let output = compile_and_run(
        r#"
import { listen, connect } from "bun";

const size = 131072;
const payload = Buffer.alloc(size, 120);
let received = 0;
let reply = "";
let firstWrite = -1;
let finalOffset = 0;
let drainCount = 0;
let bytesWritten = 0;
let finish: any;
const done = new Promise<void>((resolve) => { finish = resolve; });

function send(socket: any) {
  while (socket.data.offset < payload.length) {
    const offset = socket.data.offset;
    const written = socket.write(payload, offset, payload.length - offset);
    if (socket.data.first < 0) socket.data.first = written;
    if (written <= 0) return;
    socket.data.offset += written;
  }
  socket.end();
}

const server = listen({
  hostname: "127.0.0.1",
  port: 0,
  socket: {
    data(socket: any, bytes: Buffer) {
      received += bytes.length;
      if (received === size) {
        socket.write("ok");
        socket.end();
      }
    },
    error(_socket: any, error: Error) { throw error; },
  },
});

const client = await connect({
  hostname: "127.0.0.1",
  port: server.port,
  data: { offset: 0, first: -1, drains: 0 },
  socket: {
    open(socket: any) { send(socket); },
    drain(socket: any) {
      socket.data.drains++;
      send(socket);
    },
    data(_socket: any, bytes: Buffer) { reply += bytes.toString(); },
    close(socket: any, error: Error | undefined) {
      if (error) throw error;
      firstWrite = socket.data.first;
      finalOffset = socket.data.offset;
      drainCount = socket.data.drains;
      bytesWritten = socket.bytesWritten;
      finish();
    },
    error(_socket: any, error: Error) { throw error; },
  },
});

await done;
console.log(firstWrite, finalOffset, drainCount);
console.log(received, reply, bytesWritten);
server.stop(true);
"#,
    );
    assert_eq!(output, "65536 131072 1\n131072 ok 131072\n");
}

#[test]
fn pause_resume_terminate_and_connect_errors_have_deterministic_order() {
    let output = compile_and_run(
        r#"
import { listen, connect } from "bun";

let pausedReply = "";
let beforeResume = "unset";
let pauseOrder = "";
let refResults = "";
let finishPaused: any;
const pausedDone = new Promise<void>((resolve) => { finishPaused = resolve; });
const server = listen({
  hostname: "127.0.0.1",
  port: 0,
  socket: {
    data(socket: any, bytes: Buffer) {
      if (bytes.toString() === "pause") socket.end("buffered");
    },
    error(_socket: any, _error: Error) {},
  },
});
refResults = typeof server.unref() + "," + typeof server.ref();

await connect({
  hostname: "127.0.0.1",
  port: server.port,
  socket: {
    open(socket: any) {
      pauseOrder += "open,";
      socket.pause();
      refResults += "," + typeof socket.unref() + "," + typeof socket.ref();
      socket.write("pause");
      socket.end();
      setTimeout(() => {
        beforeResume = pausedReply;
        socket.resume();
      }, 20);
    },
    data(_socket: any, bytes: Buffer) {
      pauseOrder += "data,";
      pausedReply += bytes.toString();
    },
    end(_socket: any) { pauseOrder += "end,"; },
    close(_socket: any, error: Error | undefined) {
      if (error) throw error;
      pauseOrder += "close";
      finishPaused();
    },
    error(_socket: any, error: Error) { throw error; },
  },
});
await pausedDone;
console.log(beforeResume === "", pausedReply, pauseOrder, refResults);

let terminateOrder = "";
let finishTerminate: any;
const terminateDone = new Promise<void>((resolve) => { finishTerminate = resolve; });
await connect({
  hostname: "127.0.0.1",
  port: server.port,
  socket: {
    open(socket: any) {
      terminateOrder += "open,";
      refResults = typeof socket.terminate();
    },
    close(_socket: any, _error: Error | undefined) {
      terminateOrder += "close";
      finishTerminate();
    },
    error(_socket: any, _error: Error) {},
  },
});
await terminateDone;
console.log(terminateOrder, refResults);

const closedPort = server.port;
server.stop(true);
await new Promise<void>((resolve) => setTimeout(resolve, 20));
const errorOrder: string[] = [];
try {
  await connect({
    hostname: "127.0.0.1",
    port: closedPort,
    socket: {
      connectError(_socket: any, error: Error) {
        errorOrder.push("connectError:" + String(error.message.length > 0));
      },
      close(_socket: any, error: Error | undefined) {
        errorOrder.push("close:" + String(error !== undefined));
      },
      error(_socket: any, _error: Error) { errorOrder.push("error"); },
    },
  });
} catch (error: any) {
  errorOrder.push("reject:" + String(error.message.length > 0));
}
await new Promise<void>((resolve) => setTimeout(resolve, 20));
console.log(errorOrder.join(","));
"#,
    );
    assert_eq!(
        output,
        "true buffered open,data,end,close undefined,undefined,undefined,undefined\n\
open,close undefined\n\
connectError:true,close:true,reject:true\n"
    );
}

#[cfg(unix)]
#[test]
fn unix_socket_round_trip_uses_the_unix_option() {
    let dir = tempfile::tempdir().expect("socket tempdir");
    let socket_path = dir.path().join("bun.sock");
    let source = format!(
        r#"
import {{ listen, connect }} from "bun";

const path = {path:?};
let received = "";
let finish: any;
const done = new Promise<void>((resolve) => {{ finish = resolve; }});
const server = listen({{
  unix: path,
  socket: {{
    data(socket: any, bytes: Buffer) {{
      received += bytes.toString();
      socket.end("unix-ok");
    }},
    error(_socket: any, error: Error) {{ throw error; }},
  }},
}});
const client = await connect({{
  unix: path,
  socket: {{
    open(socket: any) {{ socket.end("hello"); }},
    data(_socket: any, bytes: Buffer) {{ console.log(bytes.toString()); }},
    close(_socket: any, error: Error | undefined) {{
      if (error) throw error;
      finish();
    }},
    error(_socket: any, error: Error) {{ throw error; }},
  }},
}});
await done;
console.log(server.unix === path, received, client.data === undefined);
server.stop(true);
"#,
        path = socket_path.to_string_lossy()
    );
    assert_eq!(compile_and_run(&source), "unix-ok\ntrue hello true\n");
}

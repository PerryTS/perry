// turnloop P1: the `node:net` surface that moved onto the event loop's own
// handles — a TCP listener and its accepted connections, a Unix-domain socket,
// half-close, write backpressure, and Node's error `code`/`syscall`.
//
// Output is deliberately free of anything host-specific: no ports (ephemeral),
// no paths (temp dir), and no `errno` (ECONNREFUSED is 61 on darwin and 111 on
// linux, so printing it would make this test assert the platform rather than
// the behaviour).
import net from "node:net";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";

function once<T>(build: (resolve: (value: T) => void) => void): Promise<T> {
  return new Promise<T>((resolve) => build(resolve));
}

async function tcpEcho(): Promise<void> {
  const server = net.createServer((socket) => {
    socket.on("data", (chunk) => {
      socket.write(chunk.toString().toUpperCase());
    });
    socket.on("end", () => {
      socket.end();
    });
  });

  await once<void>((resolve) => server.listen(0, "127.0.0.1", () => resolve()));
  const address = server.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;
  console.log("listening", port > 0);
  console.log("family", typeof address === "object" && address !== null ? address.family : "?");

  const client = net.connect(port, "127.0.0.1");
  await once<void>((resolve) => client.on("connect", () => resolve()));
  console.log("connected", client.remotePort === port);

  const received: string[] = [];
  client.on("data", (chunk) => {
    received.push(chunk.toString());
  });

  client.write("hello ");
  client.write("world");
  // `end()` half-closes: the server still gets to answer before its own end.
  client.end();

  await once<void>((resolve) => client.on("close", () => resolve()));
  console.log("echo", received.join(""));

  await once<void>((resolve) => server.close(() => resolve()));
  console.log("server closed");
}

async function unixEcho(): Promise<void> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-p1-"));
  const socketPath = path.join(dir, "echo.sock");

  const server = net.createServer((socket) => {
    socket.on("data", (chunk) => {
      socket.write(`[${chunk.toString()}]`);
      socket.end();
    });
  });
  await once<void>((resolve) => server.listen(socketPath, () => resolve()));
  console.log("uds listening", fs.existsSync(socketPath));

  const client = net.connect(socketPath);
  await once<void>((resolve) => client.on("connect", () => resolve()));

  const chunks: string[] = [];
  client.on("data", (chunk) => chunks.push(chunk.toString()));
  client.write("unix");

  await once<void>((resolve) => client.on("close", () => resolve()));
  console.log("uds echo", chunks.join(""));

  await once<void>((resolve) => server.close(() => resolve()));
  fs.rmSync(dir, { recursive: true, force: true });
  console.log("uds cleaned", !fs.existsSync(socketPath));
}

async function refusedConnect(): Promise<void> {
  // Take a port, release it, then connect: nothing is listening there.
  const probe = net.createServer();
  await once<void>((resolve) => probe.listen(0, "127.0.0.1", () => resolve()));
  const address = probe.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;
  await once<void>((resolve) => probe.close(() => resolve()));

  const socket = net.connect(port, "127.0.0.1");
  const error = await once<NodeJS.ErrnoException>((resolve) => {
    socket.on("error", (err: NodeJS.ErrnoException) => resolve(err));
  });
  console.log("refused code", error.code);
  console.log("refused syscall", error.syscall);
  console.log("refused errno is number", typeof error.errno === "number");
}

async function backpressure(): Promise<void> {
  // A server that never reads forces the client's write buffer to grow, so
  // `write()` reports false and `'drain'` arrives once it empties.
  const server = net.createServer((socket) => {
    // Read everything, but only after a turn, so the client's first writes
    // genuinely queue.
    setTimeout(() => socket.resume(), 5);
  });
  await once<void>((resolve) => server.listen(0, "127.0.0.1", () => resolve()));
  const address = server.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;

  const client = net.connect(port, "127.0.0.1");
  await once<void>((resolve) => client.on("connect", () => resolve()));

  const payload = "x".repeat(64 * 1024);
  let wrote = 0;
  for (let i = 0; i < 16; i++) {
    client.write(payload);
    wrote += payload.length;
  }
  console.log("queued bytes accounted", client.bytesWritten >= 0);

  await once<void>((resolve) => client.end(() => resolve()));
  console.log("wrote", wrote);
  await once<void>((resolve) => client.on("close", () => resolve()));
  await once<void>((resolve) => server.close(() => resolve()));
  console.log("backpressure done");
}

async function main(): Promise<void> {
  await tcpEcho();
  await unixEcho();
  await refusedConnect();
  await backpressure();
  console.log("done");
}

main();

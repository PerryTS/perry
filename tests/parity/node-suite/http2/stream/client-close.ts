import * as http2 from "node:http2";
import * as net from "node:net";

// A client stream emits 'close' once, after its terminal event: after 'end'
// for a completed response, and after 'error' for a request the session
// cancelled because its connection was refused (PerryTS/perry#15376).
const server = http2.createServer();
server.on("stream", (stream) => {
  stream.respond({ ":status": 200 });
  stream.end("ok");
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as any).port;

const completed: string[] = [];
await new Promise<void>((resolve) => {
  const client = http2.connect(`http://127.0.0.1:${port}`);
  const request = client.request({ ":path": "/" });
  request.on("response", (headers: any) => completed.push(`response ${headers[":status"]}`));
  request.on("data", (chunk: any) => completed.push(`data ${String(chunk)}`));
  request.on("end", () => completed.push("end"));
  request.on("close", () => {
    completed.push(`close rstCode=${request.rstCode} closed=${request.closed} destroyed=${request.destroyed}`);
    client.close();
    resolve();
  });
  request.end();
});
server.close();
console.log("completed:\n  " + completed.join("\n  "));

const probe = net.createServer();
await new Promise<void>((resolve) => probe.listen(0, "127.0.0.1", resolve));
const closedPort = (probe.address() as any).port;
await new Promise<void>((resolve) => probe.close(() => resolve()));
const cancelled: string[] = [];
await new Promise<void>((resolve) => {
  const client = http2.connect(`http://127.0.0.1:${closedPort}`);
  client.on("error", () => cancelled.push("session error"));
  client.on("close", () => {
    cancelled.push("session close");
    resolve();
  });
  const request = client.request({ ":path": "/" });
  request.on("error", (error: any) => cancelled.push(`stream error ${error.code}`));
  request.on("close", () => cancelled.push("stream close"));
});
console.log("cancelled:\n  " + cancelled.join("\n  "));

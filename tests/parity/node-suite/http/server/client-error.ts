import http from "node:http";
import net from "node:net";

const send = (port: number, payload: string, half = false) =>
  new Promise<string>((resolve) => {
    const client = net.connect(port, "127.0.0.1", () => {
      if (half) client.end(payload, "latin1");
      else client.write(payload, "latin1");
    });
    let received = "";
    client.on("data", (chunk) => (received += chunk.toString()));
    client.on("error", () => {});
    client.on("close", () => resolve(received));
  });

const inputs: [string, string, boolean][] = [
  ["not HTTP", "GARBAGE\r\n\r\n", false],
  ["bad version", "GET / HTTP/9.9\r\nHost: x\r\n\r\n", false],
  ["bad protocol", "GET / FOO/1.1\r\n\r\n", false],
  ["bad url", "GET /\x01 HTTP/1.1\r\nHost: x\r\n\r\n", false],
  ["bad header", "GET / HTTP/1.1\r\nHost: x\r\nb ad: 1\r\n\r\n", false],
  ["oversized header", `GET / HTTP/1.1\r\nHost: x\r\nX-Big: ${"a".repeat(20000)}\r\n\r\n`, false],
  ["half head", "GET / HT", true],
];

const run = (server: http.Server) =>
  new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", async () => {
      const { port } = server.address() as net.AddressInfo;
      for (const [label, payload, half] of inputs) {
        console.log(`${label}: ${JSON.stringify(await send(port, payload, half))}`);
      }
      server.close(() => resolve());
    });
  });

const handler = (_req: http.IncomingMessage, res: http.ServerResponse) => res.end("ok");

console.log("-- no listener");
await run(http.createServer(handler));

console.log("-- listener");
const server = http.createServer(handler);
server.on("clientError", function (this: http.Server, error: any, socket: any) {
  console.log("clientError", error.code, "|", error.message, "|", error.reason);
  console.log(error instanceof Error, this === server, socket instanceof net.Socket, socket.writable);
  if (error.code !== "HPE_INVALID_EOF_STATE") console.log(Buffer.isBuffer(error.rawPacket));
  socket.end("HTTP/1.1 400 Bad Request\r\nContent-Length: 13\r\nConnection: close\r\n\r\nfrom listener");
});
await run(server);

console.log("-- once, then the default answer");
const once = http.createServer(handler);
once.once("clientError", (error: any, socket) => {
  console.log("once", error.code);
  socket.destroy();
});
await run(once);

console.log("-- destroyed with the error");
const destroying = http.createServer(handler);
destroying.on("clientError", (error, socket) => socket.destroy(error));
await run(destroying);

console.log("-- answered later");
const later = http.createServer(handler);
later.on("clientError", (_error, socket) => setTimeout(() => socket.end("later"), 10));
await run(later);

console.log("-- request timeout");
const slow = http.createServer({ headersTimeout: 100, requestTimeout: 100, connectionsCheckingInterval: 50 }, handler);
slow.once("clientError", (error: any, socket) => {
  console.log("timeout", error.code, error.message);
  socket.end("HTTP/1.1 408 Request Timeout\r\nConnection: close\r\n\r\n");
});
slow.listen(0, "127.0.0.1", async () => {
  const { port } = slow.address() as net.AddressInfo;
  console.log(JSON.stringify(await send(port, "GET / HTTP/1.1\r\n")));
  slow.close();
});

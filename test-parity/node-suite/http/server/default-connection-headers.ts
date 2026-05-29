// #2132 — Node's HTTP/1.1 server appends `Connection: keep-alive` and
// `Keep-Alive: timeout=<keepAliveTimeout/1000>` to responses on a kept-alive
// connection, and `Connection: close` when the connection is closing. Perry
// serializes via hyper, which omits these headers (keep-alive is implicit on
// the wire), so any client reading `res.headers.connection` /
// `res.headers['keep-alive']` saw them missing. This pins the parity.

import { createServer, get } from "node:http";

const PORT = 19044;
const sockets: any[] = [];

const server = createServer((req: any, res: any) => {
  if (req.url === "/explicit-close") {
    res.setHeader("Connection", "close");
  }
  res.end("ok");
});

function probe(path: string, headers: any): Promise<void> {
  return new Promise((resolve) => {
    const req = get(
      { hostname: "127.0.0.1", port: PORT, path, headers },
      (res: any) => {
        res.on("data", () => {});
        res.on("end", () => {
          const h = res.headers;
          console.log(
            `${path} -> connection=${h.connection} keep-alive=${h["keep-alive"]}`
          );
          resolve();
        });
      }
    );
    req.on("socket", (s: any) => sockets.push(s));
  });
}

server.listen(PORT, async () => {
  // Default HTTP/1.1 request: server keeps the connection alive.
  await probe("/", {});
  // Client asks to close: server echoes Connection: close, no Keep-Alive.
  await probe("/close", { Connection: "close" });
  // Handler set its own Connection header: respected, not overridden.
  await probe("/explicit-close", {});
  // Drop any lingering keep-alive sockets so the process exits promptly.
  for (const s of sockets) s.destroy();
  server.close(() => console.log("closed"));
});

setTimeout(() => {}, 1500);

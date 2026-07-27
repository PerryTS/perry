// #2132 — Node's HTTP/1.1 server appends `Connection: keep-alive` and
// `Keep-Alive: timeout=<keepAliveTimeout/1000>` to responses on a kept-alive
// connection, and `Connection: close` when the connection is closing. Perry
// serializes via hyper, which omits these headers (keep-alive is implicit on
// the wire), so any client reading `res.headers.connection` /
// `res.headers['keep-alive']` saw them missing. This pins the parity.

import { createServer, get } from "node:http";

const sockets: any[] = [];

const server = createServer((req: any, res: any) => {
  if (req.url === "/explicit-close") {
    res.setHeader("Connection", "close");
  }
  res.end("ok");
});

function probe(path: string, headers: any): Promise<void> {
  return new Promise((resolve, reject) => {
    const address = server.address();
    if (!address || typeof address === "string") {
      throw new Error("missing address");
    }
    const req = get(
      { hostname: "127.0.0.1", port: address.port, path, headers },
      (res: any) => {
        res.on("data", () => {});
        res.once("error", reject);
        res.on("end", () => {
          const h = res.headers;
          console.log(
            `${path} -> connection=${h.connection} keep-alive=${
              h["keep-alive"]
            }`,
          );
          resolve();
        });
      },
    );
    req.on("socket", (s: any) => sockets.push(s));
    req.once("error", reject);
  });
}

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});

try {
  // Default HTTP/1.1 request: server keeps the connection alive.
  await probe("/", {});
  // Client asks to close: server echoes Connection: close, no Keep-Alive.
  await probe("/close", { Connection: "close" });
  // Handler set its own Connection header: respected, not overridden.
  await probe("/explicit-close", {});
} finally {
  for (const s of sockets) s.destroy();
  await new Promise<void>((resolve) => {
    server.close(() => {
      console.log("closed");
      resolve();
    });
  });
}

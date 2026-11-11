// Phase 1 acceptance — req.on('data') + req.on('end') body
// collection (the canonical Express body-parse pattern). The
// IncomingMessage handle's synchronous emit-on-listener-registration
// semantics mean both `'data'` and `'end'` fire as the listeners
// are added, before the handler returns; the await on the wrapping
// Promise resolves on the next microtask.

import { createServer } from "node:http";

const port = 18878;

const server = createServer((req: any, res: any) => {
  const method = req.method;
  const url = req.url;

  if (method === "POST" && url === "/echo") {
    let chunks: string[] = [];
    req.on("data", (chunk: string) => {
      chunks.push(chunk);
    });
    req.on("end", () => {
      const body = chunks.join("");
      res.statusCode = 200;
      res.setHeader("Content-Type", "text/plain");
      res.setHeader("X-Body-Bytes", String(body.length));
      res.end("got:" + body);
    });
    return;
  }

  res.statusCode = 405;
  res.end("method not allowed");
});

console.log("[node:http POST] starting on " + port);
server.listen(port);

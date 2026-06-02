// Phase 1 acceptance fixture for issue #577 — exercises the
// idiomatic Node `node:http` shape: namespace-import createServer,
// property-style `req.method` / `req.url`, property-set
// `res.statusCode = N`, method-call `res.setHeader(...)` /
// `res.end(...)`. End-to-end smoke for the HIR/codegen plumbing
// through to perry-ext-http-server's hyper accept loop.

import { createServer } from "node:http";

const port = 18877;

const server = createServer((req: any, res: any) => {
  const method = req.method;
  const url = req.url;
  console.log("[node:http test] " + method + " " + url);

  if (url === "/health") {
    res.statusCode = 200;
    res.setHeader("Content-Type", "application/json");
    res.end('{"ok":true,"path":"/health"}');
    return;
  }
  if (url === "/echo-method") {
    res.statusCode = 200;
    res.setHeader("Content-Type", "text/plain");
    res.end(method);
    return;
  }
  res.statusCode = 404;
  res.setHeader("Content-Type", "text/plain");
  res.end("not found");
});

console.log("[node:http test] starting on " + port);
server.listen(port);

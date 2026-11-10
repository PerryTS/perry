// turnloop server A/B subject (scripts/turnloop/server_ab.py): a minimal
// node:http server. `import 'fastify'` is refused under PERRY_NO_AUTO_OPTIMIZE,
// and the A/B arms are only valid with prebuilt archives, so the harness uses
// this node:http app (served by perry-ext-http).
//
// - PORT selects the port (default 18080).
// - keepAliveTimeout is set LARGE, not 0, so idle keep-alive sockets survive the
//   capacity test. Node reads 0 as "never time out"; Perry's node:http reads it
//   as "no keep-alive" and answers `Connection: close`, which closed every
//   connection the moment it was opened and made the idle test measure nothing.
//   Measured 2026-09-15: with the setter dropped or set to 600000 the response
//   carries `Connection: keep-alive` and the socket is still reusable after 4 s;
//   with `= 0` it carries `Connection: close`.
// - SIGTERM exits through process.exit, so the runtime's exit funnel prints the
//   PERRY_LOOP_STATS lines the harness collects.
import http from "node:http";

const port = parseInt(process.env.PORT || "18080", 10);
const body = "hello\n";

const server = http.createServer((_req, res) => {
  res.writeHead(200, {
    "Content-Type": "text/plain",
    "Content-Length": String(body.length),
  });
  res.end(body);
});
server.keepAliveTimeout = 600_000;

process.on("SIGTERM", () => process.exit(0));

server.listen(port, "127.0.0.1", () => {
  console.log(`listening ${port}`);
});

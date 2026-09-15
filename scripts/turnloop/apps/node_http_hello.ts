// turnloop server A/B subject (scripts/turnloop/server_ab.py): a minimal
// node:http server. `import 'fastify'` is refused under PERRY_NO_AUTO_OPTIMIZE,
// and the A/B arms are only valid with prebuilt archives, so the harness uses
// this node:http app (served by perry-ext-http).
//
// - PORT selects the port (default 18080).
// - keepAliveTimeout = 0 keeps idle keep-alive sockets open for the
//   idle-connection capacity test (Node's default reaps them after 5 s).
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
server.keepAliveTimeout = 0;

process.on("SIGTERM", () => process.exit(0));

server.listen(port, "127.0.0.1", () => {
  console.log(`listening ${port}`);
});

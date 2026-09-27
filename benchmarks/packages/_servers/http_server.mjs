// Upstream HTTP server for the axios workloads. Always run under Node (the
// same server for every arm), so only the client side differs.
//   node http_server.mjs            -> prints "listening <port>"
import http from "node:http";

const server = http.createServer((req, res) => {
  if (req.method === "GET" && req.url.startsWith("/json")) {
    const i = Number(new URL(req.url, "http://x").searchParams.get("i") || "0");
    const body = JSON.stringify({ i, name: "item-" + i, tags: ["a", "b", "c"], nested: { ok: true, n: i * 3 } });
    res.writeHead(200, { "content-type": "application/json", "content-length": Buffer.byteLength(body) });
    res.end(body);
    return;
  }
  if (req.method === "POST" && req.url === "/echo") {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const body = Buffer.concat(chunks);
      res.writeHead(200, { "content-type": "application/json", "content-length": body.length });
      res.end(body);
    });
    return;
  }
  res.writeHead(404);
  res.end();
});
server.keepAliveTimeout = 60000;
server.listen(Number(process.env.PORT || 0), "127.0.0.1", () => {
  console.log("listening " + server.address().port);
});
process.on("SIGTERM", () => server.close(() => process.exit(0)));

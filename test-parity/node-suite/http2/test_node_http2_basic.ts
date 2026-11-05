// Phase 3 acceptance — `http2.createSecureServer({key, cert}, handler)`
// with ALPN auto-negotiation between h2 and http/1.1 on the same
// port. Test runs with `curl --http2 --insecure` against the
// self-signed cert in /tmp/perry-https-cert/.

import { createSecureServer } from "node:http2";
import { readFileSync } from "node:fs";

const port = 18880;

const opts = {
  key: readFileSync("/tmp/perry-https-cert/key.pem", "utf8"),
  cert: readFileSync("/tmp/perry-https-cert/cert.pem", "utf8"),
};

const server = createSecureServer(opts, (req: any, res: any) => {
  console.log("[node:http2 test] " + req.method + " " + req.url + " (HTTP/" + req.httpVersion + ")");
  res.statusCode = 200;
  res.setHeader("Content-Type", "application/json");
  res.end('{"h2":"ok","path":"' + req.url + '","httpVersion":"' + req.httpVersion + '"}');
});

console.log("[node:http2 test] starting on " + port);
server.listen(port);

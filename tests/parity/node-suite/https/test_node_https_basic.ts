// Phase 2 acceptance — `https.createServer({ key, cert }, handler)`
// over a self-signed cert. Run via the wrapping shell script which
// generates the cert in /tmp/perry-https-cert/ and curls with
// `--insecure`.

import { createServer } from "node:https";
import { readFileSync } from "node:fs";

const port = 18879;

const opts = {
  key: readFileSync("/tmp/perry-https-cert/key.pem", "utf8"),
  cert: readFileSync("/tmp/perry-https-cert/cert.pem", "utf8"),
};

const server = createServer(opts, (req: any, res: any) => {
  console.log("[node:https test] " + req.method + " " + req.url);
  res.statusCode = 200;
  res.setHeader("Content-Type", "application/json");
  res.end('{"tls":"ok","path":"' + req.url + '"}');
});

console.log("[node:https test] starting on " + port);
server.listen(port);

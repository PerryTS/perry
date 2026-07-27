import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const server = https.createServer({ key, cert }, (_req, res) => res.end("ok"));
server.listen(0, "127.0.0.1", () => {
  const req = https.get({
    agent: false,
    ca: cert,
    host: "127.0.0.1",
    port: (server.address() as any).port,
  }, (res) => {
    console.log("agent:", req.agent === false);
    console.log("authorized:", req.socket.authorized);
    res.resume();
    res.on("end", () => server.close());
  });
  req.on("error", (error) => {
    console.log("error:", (error as any).code);
    server.close();
  });
});

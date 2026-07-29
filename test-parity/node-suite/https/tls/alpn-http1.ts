import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const server = https.createServer({
  key,
  cert,
  ALPNProtocols: ["http/1.1"],
}, (req, res) => {
  console.log("server:", req.socket.alpnProtocol, req.httpVersion);
  res.end();
});
server.listen(0, "127.0.0.1", () => {
  const req = https.get({
    ALPNProtocols: ["http/1.1"],
    ca: cert,
    host: "127.0.0.1",
    port: (server.address() as any).port,
  }, (res) => {
    console.log("client:", req.socket.alpnProtocol, res.httpVersion);
    res.resume();
    res.on("end", () => server.close());
  });
  req.on("error", (error) => {
    console.log("error:", (error as any).code);
    server.close();
  });
});

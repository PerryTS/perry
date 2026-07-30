import { readFileSync } from "node:fs";
import * as https from "node:https";
import * as tls from "node:tls";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const server = https.createServer({ key, cert }, (req, res) => {
  console.log(
    "server socket:",
    req.socket instanceof tls.TLSSocket,
    req.socket.encrypted,
  );
  res.end();
});
server.listen(0, "127.0.0.1", () => {
  const req = https.get({
    ca: cert,
    host: "127.0.0.1",
    port: (server.address() as any).port,
  }, (res) => {
    console.log(
      "client socket:",
      req.socket instanceof tls.TLSSocket,
      req.socket.encrypted,
    );
    res.resume();
    res.on("end", () => server.close());
  });
  req.on("error", (error) => {
    console.log("error:", (error as any).code);
    server.close();
  });
});

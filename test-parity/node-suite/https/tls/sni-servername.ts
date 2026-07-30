import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const server = https.createServer({ key, cert }, (req, res) => {
  console.log("servername:", req.socket.servername);
  res.end();
});
server.listen(0, "127.0.0.1", () => {
  https.get({
    ca: cert,
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "localhost",
  }, (res) => {
    res.resume();
    res.on("end", () => server.close());
  }).on("error", (error) => {
    console.log("error:", (error as any).code);
    server.close();
  });
});

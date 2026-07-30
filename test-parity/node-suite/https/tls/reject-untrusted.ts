import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const server = https.createServer({ key, cert }, (_req, res) => res.end());
server.listen(0, "127.0.0.1", () => {
  const req = https.get({
    host: "127.0.0.1",
    port: (server.address() as any).port,
  }, (response) => {
    console.log("response");
    response.resume();
    response.on("end", () => server.close());
  });
  req.on("error", (error: any) => {
    console.log(error.name, error.code);
    server.close();
  });
});

import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
let client: any;
const server = https.createServer({ key, cert }, (request, response) => {
  console.log("server:", request.method);
  response.end("done");
  client.destroy();
  server.closeAllConnections();
  server.close();
});
server.listen(0, "127.0.0.1", () => {
  client = https.get({
    host: "127.0.0.1",
    path: "/get",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
  });
  console.log("client:", client.writableEnded);
  client.on("error", () => {
    server.closeAllConnections();
    server.close();
  });
});

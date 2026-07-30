import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
let client: any;
let secureConnect = false;
const server = https.createServer({ key, cert }, (_request, response) => {
  console.log("secureConnect:", secureConnect);
  response.end();
  client.destroy();
  server.closeAllConnections();
  server.close();
});
server.listen(0, "127.0.0.1", () => {
  client = https.get({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
  });
  client.on("socket", (socket: any) => {
    socket.once("secureConnect", () => {
      secureConnect = true;
      console.log("authorized:", socket.authorized);
      console.log("authorizationError:", socket.authorizationError);
    });
  });
  client.on("error", (error: any) => {
    if (!secureConnect) console.log("error:", error.code);
    server.closeAllConnections();
    server.close();
  });
});

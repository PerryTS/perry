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
  console.log(
    "server:",
    request.method,
    request.url,
    request.headers["x-test"],
  );
  response.end("ok");
  client.destroy();
  server.closeAllConnections();
  server.close();
});
server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as any;
  client = https.request(`https://127.0.0.1:${port}/string?x=1`, {
    headers: { "x-test": "value" },
    rejectUnauthorized: false,
  });
  console.log("client:", client.method, client.path, client.protocol);
  client.on("error", () => {
    server.closeAllConnections();
    server.close();
  });
  client.end();
});

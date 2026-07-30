import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const agent = new https.Agent({
  ca: cert,
  keepAlive: false,
  maxCachedSessions: 0,
});
const server = https.createServer({
  key,
  cert,
  maxVersion: "TLSv1.2",
  minVersion: "TLSv1.2",
}, (_req, res) => res.end());

function makeRequest() {
  return new Promise<boolean>((resolve, reject) => {
    https.get({
      agent,
      host: "127.0.0.1",
      port: (server.address() as any).port,
    }, (res) => {
      const reused = res.socket.isSessionReused();
      res.resume();
      res.on("end", () => resolve(reused));
    }).on("error", reject);
  });
}

server.listen(0, "127.0.0.1", async () => {
  try {
    console.log("first:", await makeRequest());
    console.log("second:", await makeRequest());
  } catch (error: any) {
    console.log("error:", error.code);
  } finally {
    agent.destroy();
    server.close();
  }
});

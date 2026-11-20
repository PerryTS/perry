import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const defaultCert = readFileSync(new URL("localhost-cert.pem", fixture));
const apiCert = readFileSync(new URL("api-local-cert.pem", fixture));
const context = tls.createSecureContext({ key, cert: apiCert });
let requested = "none";
const server = tls.createServer({
  key,
  cert: defaultCert,
  SNICallback(name, callback) {
    requested = name;
    queueMicrotask(() => callback(null, context));
  },
}, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "async.api.local",
    rejectUnauthorized: false,
  });
  client.on(
    "secureConnect",
    () =>
      console.log(
        "selected:",
        requested,
        (client.getPeerCertificate() as any).subject?.CN,
      ),
  );
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on("close", () => server.close());
});

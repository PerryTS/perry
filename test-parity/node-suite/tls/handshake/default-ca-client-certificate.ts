import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
tls.setDefaultCACertificates([cert]);
let serverAuthorization: string | boolean = "none";
const server = tls.createServer({
  key,
  cert,
  requestCert: true,
  rejectUnauthorized: true,
}, (socket) => {
  serverAuthorization = socket.authorized || socket.authorizationError ||
    "none";
  socket.end();
});
server.on("tlsClientError", (err: any) => {
  serverAuthorization = err.code;
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "localhost",
    key,
    cert,
    rejectUnauthorized: false,
  });
  client.on(
    "secureConnect",
    () => console.log("client secure:", client.encrypted),
  );
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on(
    "close",
    () =>
      server.close(() =>
        console.log("server authorized:", serverAuthorization)
      ),
  );
});

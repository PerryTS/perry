import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverAuthorized = false;
let serverPeer = "none";
let clientAuthorized = false;
const server = tls.createServer({ key, cert, ca: cert, requestCert: true, rejectUnauthorized: true }, (socket) => {
  serverAuthorized = socket.authorized;
  serverPeer = (socket.getPeerCertificate() as any).subject?.CN ?? "missing";
  socket.end();
});
server.on("tlsClientError", (err: any) => { serverPeer = "error:" + err.code; });
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "localhost",
    key,
    cert,
    ca: cert,
  });
  client.on("secureConnect", () => { clientAuthorized = client.authorized; });
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on("close", () => server.close(() => {
    console.log("authorized:", clientAuthorized, serverAuthorized);
    console.log("server peer:", serverPeer);
  }));
});

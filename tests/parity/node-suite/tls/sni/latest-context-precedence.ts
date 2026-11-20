import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const localhostCert = readFileSync(new URL("localhost-cert.pem", fixture));
const apiCert = readFileSync(new URL("api-local-cert.pem", fixture));
const server = tls.createServer(
  { key, cert: apiCert },
  (socket) => socket.end(),
);
server.addContext("*.example.local", { key, cert: apiCert });
server.addContext("*.example.local", { key, cert: localhostCert });
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "one.example.local",
    rejectUnauthorized: false,
  });
  client.on("secureConnect", () => {
    console.log("selected:", (client.getPeerCertificate() as any).subject?.CN);
    client.end();
  });
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on("close", () => server.close());
});

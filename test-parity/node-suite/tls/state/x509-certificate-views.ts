import tls from "node:tls";
import { X509Certificate } from "node:crypto";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverOwn = false;
let serverPeerMissing = false;
const server = tls.createServer({ key, cert }, (socket) => {
  serverOwn = socket.getX509Certificate() instanceof X509Certificate;
  serverPeerMissing = socket.getPeerX509Certificate() === undefined;
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("secureConnect", () => {
    const legacy: any = client.getPeerCertificate();
    const x509 = client.getPeerX509Certificate();
    console.log("client peer:", x509 instanceof X509Certificate, x509?.subject.includes("CN=localhost"));
    console.log("same raw:", Buffer.isBuffer(legacy.raw), x509?.raw.equals(legacy.raw));
    console.log("client own missing:", client.getX509Certificate() === undefined);
  });
  client.on("close", () => server.close(() => console.log("server views:", serverOwn, serverPeerMissing)));
});

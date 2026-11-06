import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("secureConnect", () => {
    const abbreviated: any = client.getPeerCertificate(false);
    const detailed: any = client.getPeerCertificate(true);
    console.log("identity:", abbreviated.subject?.CN, detailed.subject?.CN);
    console.log("same raw:", abbreviated.raw.equals(detailed.raw));
    console.log("issuer:", detailed.issuerCertificate?.subject?.CN, detailed.issuerCertificate === detailed);
  });
  client.on("close", () => server.close());
});

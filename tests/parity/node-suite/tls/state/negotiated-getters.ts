import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.once("secureConnect", () => {
    const cipher: any = client.getCipher();
    const peer: any = client.getPeerCertificate();
    const session = client.getSession();
    console.log("protocol:", ["TLSv1.2", "TLSv1.3"].includes(client.getProtocol() as string));
    console.log("cipher:", typeof cipher.name, typeof cipher.standardName, typeof cipher.version);
    console.log("peer:", peer.subject?.CN, typeof peer.raw?.byteLength === "number");
    console.log("session:", Buffer.isBuffer(session), (session?.length ?? 0) > 0, client.isSessionReused());
  });
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on("close", () => server.close());
});

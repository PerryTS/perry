import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert }, (socket) => socket.end("ok"));
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, servername: "localhost", ca: cert });
  client.once("secureConnect", () => {
    const peer: any = client.getPeerCertificate();
    console.log("authorization:", client.authorized, client.authorizationError);
    console.log("peer identity:", peer.subject?.CN, peer.subjectaltname?.includes("DNS:localhost"), peer.subjectaltname?.includes("IP Address:127.0.0.1"));
  });
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on("close", () => server.close());
  client.resume();
});

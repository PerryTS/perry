import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let checked = "none";
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "mismatch.api.local",
    ca: cert,
    checkServerIdentity(hostname, peer) {
      checked = hostname + "/" + peer.subject.CN;
      return undefined;
    },
  });
  client.on(
    "secureConnect",
    () => console.log("accepted:", client.authorized, checked),
  );
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on("close", () => server.close());
});

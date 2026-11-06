import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const secureContext = tls.createSecureContext({ ca: cert, minVersion: "TLSv1.2" });
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "localhost",
    secureContext,
  });
  client.on("secureConnect", () => {
    console.log("authorized:", client.authorized, client.authorizationError);
    console.log("protocol:", ["TLSv1.2", "TLSv1.3"].includes(client.getProtocol() as string));
  });
  client.on("error", (err: any) => console.log("error:", err.code));
  client.on("close", () => server.close());
});

import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert, minVersion: "TLSv1.2", maxVersion: "TLSv1.2" }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => connectOnce());
function connectOnce(session?: Buffer) {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false, minVersion: "TLSv1.2", maxVersion: "TLSv1.2", session });
  client.on("secureConnect", () => {
    if (!session) {
      const first = client.getSession();
      console.log("first:", client.isSessionReused(), Buffer.isBuffer(first), (first?.length ?? 0) > 0);
      client.once("close", () => connectOnce(first));
    } else {
      console.log("second:", client.isSessionReused());
      client.once("close", () => server.close());
    }
    client.end();
  });
  client.on("error", (err: any) => {
    console.log("error:", err.code);
    server.close();
  });
}

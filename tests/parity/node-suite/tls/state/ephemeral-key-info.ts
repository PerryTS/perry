import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverInfoIsNull = false;
const cipher = "ECDHE-RSA-AES128-GCM-SHA256";
const server = tls.createServer({
  key,
  cert,
  ciphers: cipher,
  minVersion: "TLSv1.2",
  maxVersion: "TLSv1.2",
}, (socket) => {
  serverInfoIsNull = socket.getEphemeralKeyInfo() === null;
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
    ciphers: cipher,
    minVersion: "TLSv1.2",
    maxVersion: "TLSv1.2",
  });
  client.on("secureConnect", () => {
    const info: any = client.getEphemeralKeyInfo();
    console.log(
      "client shape:",
      typeof info?.type,
      typeof info?.name,
      typeof info?.size,
      info?.size > 0,
    );
  });
  client.on(
    "close",
    () => server.close(() => console.log("server null:", serverInfoIsNull)),
  );
});

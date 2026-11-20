import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let secure = false;
let errorCode = "none";
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "localhost",
    ca: cert,
    checkServerIdentity() {
      return Object.assign(new Error("identity rejected"), {
        code: "CUSTOM_IDENTITY",
      });
    },
  });
  client.on("secureConnect", () => {
    secure = true;
    client.destroy();
  });
  client.on("error", (err: any) => {
    errorCode = err.code;
  });
  client.on(
    "close",
    () => server.close(() => console.log("result:", secure, errorCode)),
  );
});

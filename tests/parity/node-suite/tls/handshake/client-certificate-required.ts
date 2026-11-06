import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let secure = false;
let clientError = false;
let serverError = false;
let serverAccepted = false;
const options = {
  minVersion: "TLSv1.2" as const,
  maxVersion: "TLSv1.2" as const,
};
const server = tls.createServer({
  key,
  cert,
  ca: cert,
  requestCert: true,
  rejectUnauthorized: true,
  ...options,
}, (socket) => {
  serverAccepted = true;
  socket.destroy();
});
server.on("tlsClientError", (_err, socket) => {
  serverError = true;
  socket.destroy();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
    ...options,
  });
  client.on("secureConnect", () => {
    secure = true;
    client.destroy();
  });
  client.on("error", () => {
    clientError = true;
  });
  client.on(
    "close",
    () =>
      server.close(() =>
        console.log("result:", secure, clientError, serverError, serverAccepted)
      ),
  );
});

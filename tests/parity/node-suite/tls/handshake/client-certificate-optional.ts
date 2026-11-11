import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverState = "none";
function authorizationCode(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object") {
    const error = value as { code?: unknown; name?: unknown };
    return String(error.code ?? error.name ?? "error");
  }
  return "none";
}
const server = tls.createServer({
  key,
  cert,
  ca: cert,
  requestCert: true,
  rejectUnauthorized: false,
}, (socket) => {
  serverState = [
    socket.authorized,
    authorizationCode(socket.authorizationError),
    Object.keys(socket.getPeerCertificate()).length,
  ].join("/");
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
  });
  client.on(
    "secureConnect",
    () => console.log("client secure:", client.encrypted),
  );
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on(
    "close",
    () => server.close(() => console.log("server:", serverState)),
  );
});

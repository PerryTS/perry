import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const context = tls.createSecureContext({ key, cert });
let requested = "none";
const server = tls.createServer({ key, cert, SNICallback(name, callback) {
  requested = name;
  callback(null, context);
} }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, servername: "api.local", rejectUnauthorized: false });
  client.on("secureConnect", () => console.log("client servername:", client.servername));
  client.on("close", () => server.close(() => console.log("callback name:", requested)));
});

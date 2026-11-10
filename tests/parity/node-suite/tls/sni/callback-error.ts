import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverMessage = "none";
let serverName = "none";
let clientError = false;
const server = tls.createServer({
  key,
  cert,
  SNICallback(name, callback) {
    callback(new Error("selection failed"));
  },
});
server.on("tlsClientError", (err, socket) => {
  serverMessage = err.message;
  serverName = socket.servername;
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "failed.api.local",
    rejectUnauthorized: false,
  });
  client.on("secureConnect", () => client.destroy());
  client.on("error", () => {
    clientError = true;
  });
  client.on("close", () =>
    server.close(() => {
      console.log("client error:", clientError);
      console.log("server:", serverMessage, serverName);
    }));
});

import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverError = "none";
let clientError = false;
let secure = false;
const server = tls.createServer({ key, cert, ALPNCallback: () => "unoffered" });
server.on("tlsClientError", (err: any, socket) => {
  serverError = err.code;
  socket.destroy();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
    ALPNProtocols: ["h2", "http/1.1"],
  });
  client.on("secureConnect", () => {
    secure = true;
    client.destroy();
  });
  client.on("error", () => {
    clientError = true;
  });
  client.on("close", () =>
    server.close(() => {
      console.log("secure:", secure);
      console.log("client error:", clientError);
      console.log("server error:", serverError);
    }));
});

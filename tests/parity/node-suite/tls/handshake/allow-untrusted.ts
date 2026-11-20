import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "localhost",
    rejectUnauthorized: false,
  });
  client.on("secureConnect", () => {
    console.log("connected:", client.encrypted);
    console.log("authorization:", client.authorized, client.authorizationError);
  });
  client.on("close", () => server.close());
});

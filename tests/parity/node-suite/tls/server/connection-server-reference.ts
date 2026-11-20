import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let rawReference = false;
let secureReference = false;
const server = tls.createServer({ key, cert }, (socket: any) => {
  secureReference = socket.server === server;
  socket.end();
});
server.on("connection", (socket: any) => { rawReference = socket.server === server; });
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("close", () => server.close(() => console.log("references:", rawReference, secureReference)));
});

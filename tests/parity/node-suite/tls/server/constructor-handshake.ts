import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let accepted = false;
const server = new tls.Server({ key, cert }, (socket) => {
  accepted = socket instanceof tls.TLSSocket;
  socket.end();
});
console.log("server class:", server instanceof tls.Server);
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("close", () => server.close(() => console.log("accepted:", accepted)));
});

import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverFinished: Buffer | undefined;
let serverPeerFinished: Buffer | undefined;
let clientFinished: Buffer | undefined;
let clientPeerFinished: Buffer | undefined;
const server = tls.createServer({ key, cert }, (socket) => {
  serverFinished = socket.getFinished();
  serverPeerFinished = socket.getPeerFinished();
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("secureConnect", () => {
    clientFinished = client.getFinished();
    clientPeerFinished = client.getPeerFinished();
  });
  client.on("close", () => server.close(() => {
    console.log("buffers:", [serverFinished, serverPeerFinished, clientFinished, clientPeerFinished].every(Buffer.isBuffer));
    console.log("nonempty:", [serverFinished, serverPeerFinished, clientFinished, clientPeerFinished].every((value) => (value?.length ?? 0) > 0));
    console.log("cross match:", serverFinished?.equals(clientPeerFinished as Buffer), serverPeerFinished?.equals(clientFinished as Buffer));
  }));
});

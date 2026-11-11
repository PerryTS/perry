import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const label = "perry-node-suite";
let serverMaterial: Buffer | undefined;
let clientMaterial: Buffer | undefined;
let clientWithContext: Buffer | undefined;
const server = tls.createServer({ key, cert }, (socket) => {
  serverMaterial = socket.exportKeyingMaterial(32, label);
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("secureConnect", () => {
    clientMaterial = client.exportKeyingMaterial(32, label);
    clientWithContext = client.exportKeyingMaterial(32, label, Buffer.from([0, 1, 2, 3]));
  });
  client.on("close", () => server.close(() => {
    console.log("buffers:", [serverMaterial, clientMaterial, clientWithContext].every(Buffer.isBuffer));
    console.log("lengths:", serverMaterial?.length, clientMaterial?.length, clientWithContext?.length);
    console.log("agreement:", serverMaterial?.equals(clientMaterial as Buffer));
    console.log("context differs:", !clientMaterial?.equals(clientWithContext as Buffer));
  }));
});

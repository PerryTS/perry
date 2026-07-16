import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect(
    (server.address() as any).port,
    "127.0.0.1",
    { rejectUnauthorized: false },
    function () {
      console.log("callback receiver:", this === client);
      console.log("socket class:", client instanceof tls.TLSSocket, client.encrypted);
    },
  );
  client.on("close", () => server.close());
});

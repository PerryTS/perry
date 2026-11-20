import tls from "node:tls";
import net from "node:net";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert }, (socket) => socket.end("factory"));
let raw: any;
let client: any;
const deadline = setTimeout(() => {
  console.log("deadline");
  client?.destroy();
  raw?.destroy();
  server.close();
  process.exitCode = 1;
}, 4000);
server.listen(0, "127.0.0.1", () => {
  // Keep the existing public transport and handshake shape as the control.
  const parent = net.connect((server.address() as any).port, "127.0.0.1", () => {
    raw = parent;
    client = tls.connect({ socket: parent, rejectUnauthorized: false });
    Object.defineProperty(client, "stableProbe", { value: false, writable: true });
    console.log("assigned:", client.stableProbe, typeof client.setEncoding);
    client.stableProbe = true;
    console.log("write:", client.stableProbe, client.setEncoding("utf8") === client);
    let data = "";
    client.on("secureConnect", () => console.log("secure:", client.stableProbe));
    client.on("data", (chunk) => { data += chunk.toString(); });
    client.on("error", (error: any) => console.log("client error:", error.code));
    client.on("end", () => console.log("data:", data));
    client.on("close", () => {
      clearTimeout(deadline);
      server.close();
    });
  });
  parent.on("error", (error: any) => {
    console.log("raw error:", error.code);
    clearTimeout(deadline);
    server.close();
  });
});

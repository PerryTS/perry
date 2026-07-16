import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let offered = "none";
let serverProtocol: string | false | null = null;
const server = tls.createServer({
  key,
  cert,
  ALPNCallback({ protocols }) {
    offered = protocols.join(",");
    return protocols.includes("h2") ? "h2" : undefined;
  },
}, (socket) => {
  serverProtocol = socket.alpnProtocol;
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
    ALPNProtocols: ["http/1.1", "h2"],
  });
  client.on("secureConnect", () => console.log("client:", client.alpnProtocol));
  client.on("close", () => server.close(() => {
    console.log("server:", serverProtocol);
    console.log("offered:", offered);
  }));
});

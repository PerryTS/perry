import tls from "node:tls";
import net from "node:net";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let tlsError = false;
let secureConnection = false;
const server = tls.createServer({ key, cert }, () => { secureConnection = true; });
server.on("tlsClientError", (err) => { tlsError = err instanceof Error; });
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as any).port, "127.0.0.1", () => {
    client.end("GET / HTTP/1.0\r\n\r\n");
  });
  client.on("error", () => {});
  client.on("close", () => server.close(() => console.log("rejected:", tlsError, secureConnection)));
});

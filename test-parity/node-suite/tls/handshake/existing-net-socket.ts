import tls from "node:tls";
import net from "node:net";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer(
  { key, cert },
  (socket) => socket.end("wrapped"),
);
server.listen(0, "127.0.0.1", () => {
  const raw = net.connect((server.address() as any).port, "127.0.0.1", () => {
    const client = tls.connect({ socket: raw, rejectUnauthorized: false });
    let data = "";
    client.on(
      "secureConnect",
      () => console.log("secure:", client.encrypted, client.authorized),
    );
    client.on("data", (chunk) => {
      data += chunk.toString();
    });
    client.on("error", (err: any) => console.log("client error:", err.code));
    client.on("end", () => console.log("data:", data));
    client.on("close", () => server.close());
  });
  raw.on("error", (err: any) => {
    console.log("raw error:", err.code);
    server.close();
  });
});

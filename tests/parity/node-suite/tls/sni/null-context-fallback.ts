import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverName = "none";
const server = tls.createServer({
  key,
  cert,
  SNICallback(name, callback) {
    callback(null, null as any);
  },
}, (socket) => {
  serverName = socket.servername;
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "fallback.api.local",
    rejectUnauthorized: false,
  });
  client.on(
    "secureConnect",
    () =>
      console.log(
        "certificate:",
        (client.getPeerCertificate() as any).subject?.CN,
      ),
  );
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on(
    "close",
    () => server.close(() => console.log("servername:", serverName)),
  );
});

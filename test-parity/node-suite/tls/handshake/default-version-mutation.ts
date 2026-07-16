import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const originalMin = tls.DEFAULT_MIN_VERSION;
const originalMax = tls.DEFAULT_MAX_VERSION;
(tls as any).DEFAULT_MIN_VERSION = "TLSv1.2";
(tls as any).DEFAULT_MAX_VERSION = "TLSv1.2";
let restored = false;
function restoreDefaults() {
  if (restored) return;
  restored = true;
  (tls as any).DEFAULT_MIN_VERSION = originalMin;
  (tls as any).DEFAULT_MAX_VERSION = originalMax;
}
process.once("exit", restoreDefaults);
let serverProtocol: string | null = null;
const server = tls.createServer({ key, cert }, (socket) => {
  serverProtocol = socket.getProtocol();
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
  });
  client.on(
    "secureConnect",
    () => console.log("client:", client.getProtocol()),
  );
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on("close", () =>
    server.close(() => {
      console.log("server:", serverProtocol);
      restoreDefaults();
      console.log(
        "restored:",
        tls.DEFAULT_MIN_VERSION === originalMin,
        tls.DEFAULT_MAX_VERSION === originalMax,
      );
    }));
});

import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let serverShape = false;
const server = tls.createServer({ key, cert }, (socket) => {
  const algorithms = socket.getSharedSigalgs();
  serverShape = Array.isArray(algorithms) &&
    algorithms.every((value) => typeof value === "string");
  socket.end();
});
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
  });
  client.on("secureConnect", () => {
    const algorithms = client.getSharedSigalgs();
    console.log(
      "client:",
      Array.isArray(algorithms),
      algorithms.every((value) => typeof value === "string"),
    );
  });
  client.on("error", (err: any) => console.log("client error:", err.code));
  client.on(
    "close",
    () => server.close(() => console.log("server:", serverShape)),
  );
});

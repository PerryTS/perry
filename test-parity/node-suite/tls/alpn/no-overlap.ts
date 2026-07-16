import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let secure = false;
let clientError = "none";
let serverError = "none";
const server = tls.createServer({ key, cert, ALPNProtocols: ["h2"] });
server.on("tlsClientError", (err: any) => { serverError = err.code; });
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
    ALPNProtocols: ["http/1.1"],
  });
  client.on("secureConnect", () => {
    secure = true;
    client.destroy();
  });
  client.on("error", (err: any) => { clientError = err.code; });
  client.on("close", () => server.close(() => {
    console.log("secure:", secure);
    console.log("errors:", clientError, serverError);
  }));
});

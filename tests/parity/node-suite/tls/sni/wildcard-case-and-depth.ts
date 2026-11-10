import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const defaultCert = readFileSync(new URL("localhost-cert.pem", fixture));
const apiCert = readFileSync(new URL("api-local-cert.pem", fixture));
const server = tls.createServer(
  { key, cert: defaultCert },
  (socket) => socket.end(),
);
server.addContext("*.api.local", { key, cert: apiCert });
server.listen(
  0,
  "127.0.0.1",
  () =>
    connect(
      "SERVICE.API.LOCAL",
      () => connect("deep.service.api.local", () => server.close()),
    ),
);

function connect(servername: string, done: () => void) {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername,
    rejectUnauthorized: false,
  });
  client.on("secureConnect", () => {
    console.log(
      servername + ":",
      (client.getPeerCertificate() as any).subject?.CN,
    );
    client.end();
  });
  client.on(
    "error",
    (err: any) => console.log(servername + " error:", err.code),
  );
  client.on("close", done);
}

import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
tls.setDefaultCACertificates([cert]);
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(
  0,
  "127.0.0.1",
  () =>
    connect(
      "implicit",
      undefined,
      () => connect("empty", [], () => server.close()),
    ),
);

function connect(label: string, ca: Buffer[] | undefined, done: () => void) {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    servername: "localhost",
    rejectUnauthorized: false,
    ...(ca === undefined ? {} : { ca }),
  });
  client.on(
    "secureConnect",
    () =>
      console.log(
        label + ":",
        client.authorized,
        client.authorizationError ?? "none",
      ),
  );
  client.on("error", (err: any) => console.log(label + " error:", err.code));
  client.on("close", done);
}

import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const defaultCert = readFileSync(new URL("localhost-cert.pem", fixture));
const apiCert = readFileSync(new URL("api-local-cert.pem", fixture));
const values = [
  { label: "options", value: { key, cert: apiCert } },
  {
    label: "context",
    value: tls.createSecureContext({ key, cert: apiCert }),
  },
];
run(0);

function run(index: number) {
  if (index === values.length) return;
  const current = values[index];
  let callbackSocket = false;
  const server = tls.createServer({
    key,
    cert: defaultCert,
    ALPNCallback({ protocols }) {
      callbackSocket = this instanceof tls.TLSSocket;
      this.setKeyCert(current.value);
      return protocols[0];
    },
  }, (socket) => socket.end());
  server.listen(0, "127.0.0.1", () => {
    const client = tls.connect({
      host: "127.0.0.1",
      port: (server.address() as any).port,
      rejectUnauthorized: false,
      ALPNProtocols: ["acme-tls/1"],
    });
    client.on("secureConnect", () => {
      console.log(
        current.label + ":",
        callbackSocket,
        client.alpnProtocol,
        (client.getPeerCertificate() as any).subject?.CN,
      );
      client.end();
    });
    client.on(
      "error",
      (err: any) => console.log(current.label + " error:", err.code),
    );
    client.on("close", () => server.close(() => run(index + 1)));
  });
}

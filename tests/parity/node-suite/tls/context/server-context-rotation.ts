import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const localhostCert = readFileSync(new URL("localhost-cert.pem", fixture));
const apiCert = readFileSync(new URL("api-local-cert.pem", fixture));
let firstServerSocket: tls.TLSSocket | undefined;
let connection = 0;
const server = tls.createServer({ key, cert: localhostCert }, (socket) => {
  if (++connection === 1) {
    firstServerSocket = socket;
    socket.write("before|");
  } else {
    socket.end();
  }
});
server.listen(0, "127.0.0.1", () => {
  let firstData = "";
  let rotated = false;
  const first = connect((client) => {
    console.log(
      "first cert:",
      (client.getPeerCertificate() as any).subject?.CN,
    );
  });
  first.on("data", (chunk) => {
    firstData += chunk.toString();
    if (!rotated && firstData === "before|") {
      rotated = true;
      server.setSecureContext({ key, cert: apiCert });
      const second = connect((client) => {
        console.log(
          "second cert:",
          (client.getPeerCertificate() as any).subject?.CN,
        );
        client.end();
      });
      second.on("close", () => firstServerSocket?.end("after"));
    }
  });
  first.on("end", () => console.log("first data:", firstData));
  first.on("close", () => server.close());
});

function connect(onSecure: (client: tls.TLSSocket) => void) {
  const client = tls.connect({
    host: "127.0.0.1",
    port: (server.address() as any).port,
    rejectUnauthorized: false,
  });
  client.on("secureConnect", () => onSecure(client));
  client.on("error", (err: any) => console.log("client error:", err.code));
  return client;
}

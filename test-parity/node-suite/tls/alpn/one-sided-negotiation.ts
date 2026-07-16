import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
run(
  "server only",
  { ALPNProtocols: ["h2"] },
  {},
  () => run("client only", {}, { ALPNProtocols: ["h2"] }, () => {}),
);

function run(
  label: string,
  serverOptions: any,
  clientOptions: any,
  done: () => void,
) {
  let serverProtocol: string | false | null = null;
  const server = tls.createServer({ key, cert, ...serverOptions }, (socket) => {
    serverProtocol = socket.alpnProtocol;
    socket.end();
  });
  server.listen(0, "127.0.0.1", () => {
    const client = tls.connect({
      host: "127.0.0.1",
      port: (server.address() as any).port,
      rejectUnauthorized: false,
      ...clientOptions,
    });
    client.on(
      "secureConnect",
      () => console.log(label + " client:", client.alpnProtocol),
    );
    client.on("error", (err: any) => console.log(label + " error:", err.code));
    client.on("close", () =>
      server.close(() => {
        console.log(label + " server:", serverProtocol);
        done();
      }));
  });
}

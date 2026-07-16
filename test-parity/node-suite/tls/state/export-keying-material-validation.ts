import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const client = tls.connect({ host: "127.0.0.1", port: (server.address() as any).port, rejectUnauthorized: false });
  client.on("secureConnect", () => {
    const probes: Array<[string, () => unknown]> = [
      ["context string", () => client.exportKeyingMaterial(8, "label", "bad" as any)],
      ["label null", () => client.exportKeyingMaterial(8, null as any)],
      ["length string", () => client.exportKeyingMaterial("8" as any, "label")],
      ["negative length", () => client.exportKeyingMaterial(-1, "label")],
      ["zero length", () => client.exportKeyingMaterial(0, "label")],
    ];
    for (const [label, probe] of probes) {
      try {
        probe();
        console.log(label + ": no throw");
      } catch (err: any) {
        console.log(label + ":", err instanceof TypeError || err instanceof RangeError, err.code);
      }
    }
    const emptyContext = client.exportKeyingMaterial(8, "label", Buffer.alloc(0));
    console.log("empty context:", Buffer.isBuffer(emptyContext), emptyContext.length);
  });
  client.on("close", () => server.close());
});

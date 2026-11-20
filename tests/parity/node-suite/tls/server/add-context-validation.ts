import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer({ key, cert });
console.log("options:", server.addContext("options.local", { key, cert }) === undefined);
console.log("context:", server.addContext("context.local", tls.createSecureContext({ key, cert })) === undefined);
for (const [label, hostname, context] of [
  ["empty hostname", "", { key, cert }],
  ["hostname type", 1, { key, cert }],
  ["context type", "bad.local", 1],
] as const) {
  try {
    server.addContext(hostname as any, context as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError || err instanceof Error, err.code ?? "none");
  }
}

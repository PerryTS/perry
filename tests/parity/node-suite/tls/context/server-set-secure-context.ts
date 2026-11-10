import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const server = tls.createServer();
console.log("valid return:", server.setSecureContext({ key, cert }) === undefined);
console.log("empty return:", server.setSecureContext({}) === undefined);
for (const [label, value] of [["null", null], ["number", 1]] as const) {
  try {
    server.setSecureContext(value as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}

import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = new Uint8Array(readFileSync(new URL("localhost-key.pem", fixture)));
const cert = new Uint8Array(readFileSync(new URL("localhost-cert.pem", fixture)));
const cases: Array<[string, any, any]> = [
  ["uint8", key, cert],
  ["dataview", new DataView(key.buffer, key.byteOffset, key.byteLength), new DataView(cert.buffer, cert.byteOffset, cert.byteLength)],
  ["arraybuffer", key.buffer, cert.buffer],
];
for (const [label, keyValue, certValue] of cases) {
  try {
    console.log(label + ":", tls.createSecureContext({ key: keyValue, cert: certValue }) instanceof tls.SecureContext);
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}

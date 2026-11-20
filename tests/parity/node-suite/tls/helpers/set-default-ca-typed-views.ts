import tls from "node:tls";
import { readFileSync } from "node:fs";

const cert = readFileSync(new URL("../fixtures/localhost-cert.pem", import.meta.url));
const uint8 = new Uint8Array(cert);
const inputs: Array<[string, ArrayBufferView]> = [
  ["buffer", Buffer.from(cert)],
  ["uint8", uint8],
  ["dataview", new DataView(uint8.buffer, uint8.byteOffset, uint8.byteLength)],
];
for (const [label, input] of inputs) {
  console.log(label + " return:", tls.setDefaultCACertificates([input]) === undefined);
  const actual = tls.getCACertificates("default");
  console.log(label + " value:", actual.length, actual[0] === cert.toString());
}

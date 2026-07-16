import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
for (
  const [label, options] of [
    ["key object", { key: { pem: key }, cert }],
    ["key array item", { key: [key, true], cert: [cert, cert] }],
    ["cert array item", { key: [key, key], cert: [cert, true] }],
    ["ca array item", { key, cert, ca: [cert, true] }],
  ] as const
) {
  try {
    tls.createSecureContext(options as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}

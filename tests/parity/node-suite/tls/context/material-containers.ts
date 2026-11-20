import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
for (
  const [label, options] of [
    ["buffer arrays", { key: [key], cert: [cert], ca: [cert] }],
    ["string arrays", {
      key: [key.toString()],
      cert: [cert.toString()],
      ca: [cert.toString()],
    }],
    ["key object buffer", { key: [{ pem: key }], cert }],
    ["key object string", {
      key: [{ pem: key.toString() }],
      cert: cert.toString(),
    }],
  ] as const
) {
  try {
    console.log(
      label + ":",
      tls.createSecureContext(options as any) instanceof tls.SecureContext,
    );
  } catch (err: any) {
    console.log(label + ":", false, err.code ?? err.name);
  }
}

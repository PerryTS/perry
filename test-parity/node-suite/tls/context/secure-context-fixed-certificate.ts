import tls from "node:tls";
import { readFileSync } from "node:fs";

const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
for (const material of [
  { key, cert },
  { key: key.toString(), cert: cert.toString() },
  { key, cert, ca: cert },
]) {
  const context = tls.createSecureContext(material);
  console.log("context:", context instanceof tls.SecureContext, typeof context.context);
}

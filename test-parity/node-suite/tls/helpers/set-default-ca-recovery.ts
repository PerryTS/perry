import tls from "node:tls";
import { readFileSync } from "node:fs";

const cert = readFileSync(new URL("../fixtures/localhost-cert.pem", import.meta.url)).toString();
tls.setDefaultCACertificates([cert]);
try {
  tls.setDefaultCACertificates(["not a valid certificate"]);
  console.log("invalid only: no throw");
} catch (err: any) {
  console.log("invalid only:", err instanceof Error, err.code);
}
let actual = tls.getCACertificates("default");
console.log("invalid only recovery:", actual.length, actual[0] === cert);

const malformedPem = "-----BEGIN CERTIFICATE-----\nvalid cert content\n-----END CERTIFICATE-----";
try {
  tls.setDefaultCACertificates([cert, malformedPem]);
  console.log("mixed invalid: no throw");
} catch (err: any) {
  console.log("mixed invalid:", err instanceof Error, typeof err.code === "string");
}
actual = tls.getCACertificates("default");
console.log("mixed invalid recovery:", actual.length, actual[0] === cert);

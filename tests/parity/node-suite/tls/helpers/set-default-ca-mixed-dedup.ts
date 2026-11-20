import tls from "node:tls";
import { readFileSync } from "node:fs";

const cert = readFileSync(new URL("../fixtures/localhost-cert.pem", import.meta.url));
const bundled = tls.getCACertificates("bundled");
const system = tls.getCACertificates("system");
tls.setDefaultCACertificates([cert.toString(), Buffer.from(cert), new Uint8Array(cert)]);
const actual = tls.getCACertificates("default");
console.log("deduplicated:", actual.length, actual[0] === cert.toString());
console.log("implicit cache:", actual === tls.getCACertificates());
console.log("other stores:", bundled === tls.getCACertificates("bundled"), system === tls.getCACertificates("system"));

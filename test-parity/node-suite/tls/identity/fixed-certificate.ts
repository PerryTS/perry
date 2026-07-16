import tls from "node:tls";
import { X509Certificate } from "node:crypto";
import { readFileSync } from "node:fs";

const pem = readFileSync(new URL("../fixtures/localhost-cert.pem", import.meta.url));
const cert = new X509Certificate(pem);
const legacy = cert.toLegacyObject();
console.log("localhost:", tls.checkServerIdentity("localhost", legacy) === undefined);
console.log("ipv4:", tls.checkServerIdentity("127.0.0.1", legacy) === undefined);
const mismatch: any = tls.checkServerIdentity("other.local", legacy);
console.log("mismatch:", mismatch instanceof Error, mismatch.code, mismatch.host, mismatch.cert === legacy);

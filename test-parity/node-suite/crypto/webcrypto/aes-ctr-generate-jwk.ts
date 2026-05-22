import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  for (const length of [128, 192, 256]) {
    const key = await crypto.subtle.generateKey({ name: "AES-CTR", length }, true, ["encrypt", "decrypt"]);
    const jwk = await crypto.subtle.exportKey("jwk", key) as JsonWebKey;
    const imported = await crypto.subtle.importKey("jwk", jwk, "AES-CTR", true, ["encrypt", "decrypt"]);
    const raw = await crypto.subtle.exportKey("raw", imported);
    const counter = Buffer.alloc(16);
    const ct = await crypto.subtle.encrypt({ name: "AES-CTR", counter, length: 32 }, imported, new TextEncoder().encode("ctr generated"));
    const pt = await crypto.subtle.decrypt({ name: "AES-CTR", counter, length: 32 }, imported, ct);
    console.log("ctr generated:", length, Buffer.from(raw).length, jwk.kty, !!jwk.k, Buffer.from(pt).toString());
  }
}
await main();

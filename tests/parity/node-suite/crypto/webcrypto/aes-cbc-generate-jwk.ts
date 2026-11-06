import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  for (const length of [128, 192, 256]) {
    const key = await crypto.subtle.generateKey({ name: "AES-CBC", length }, true, ["encrypt", "decrypt"]);
    const jwk = await crypto.subtle.exportKey("jwk", key) as JsonWebKey;
    const imported = await crypto.subtle.importKey("jwk", jwk, "AES-CBC", true, ["encrypt", "decrypt"]);
    const raw = await crypto.subtle.exportKey("raw", imported);
    console.log("cbc generated len:", length, Buffer.from(raw).length, jwk.kty, !!jwk.k);
  }
}
await main();

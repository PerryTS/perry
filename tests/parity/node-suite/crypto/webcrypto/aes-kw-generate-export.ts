import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

for (const length of [128, 192, 256]) {
  const key = await crypto.subtle.generateKey({ name: "AES-KW", length }, true, ["wrapKey", "unwrapKey"]);
  const raw = await crypto.subtle.exportKey("raw", key);
  const jwk = await crypto.subtle.exportKey("jwk", key) as JsonWebKey;
  const imported = await crypto.subtle.importKey("jwk", jwk, "AES-KW", true, ["wrapKey", "unwrapKey"]);
  const importedRaw = await crypto.subtle.exportKey("raw", imported);
  console.log("aes-kw generated:", length, Buffer.from(raw).length, jwk.kty, !!jwk.k, Buffer.from(importedRaw).length);
}

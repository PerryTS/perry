import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const raw = Buffer.alloc(24, 9);
  const iv = Buffer.alloc(12, 3);
  const aad = Buffer.from("webcrypto aes-192-gcm aad");
  const data = new TextEncoder().encode("webcrypto aes-192-gcm parity");
  const imported = await crypto.subtle.importKey("raw", raw, "AES-GCM", true, ["encrypt", "decrypt"]);
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: aad }, imported, data);
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv, additionalData: aad }, imported, ct);
  const exportedRaw = await crypto.subtle.exportKey("raw", imported);
  const jwk = await crypto.subtle.exportKey("jwk", imported) as JsonWebKey;
  const generated = await crypto.subtle.generateKey({ name: "AES-GCM", length: 192 }, true, ["encrypt", "decrypt"]);
  const generatedRaw = await crypto.subtle.exportKey("raw", generated);
  console.log("aes-gcm-192 raw len:", Buffer.from(exportedRaw).length);
  console.log("aes-gcm-192 jwk:", jwk.kty, !!jwk.k);
  console.log("aes-gcm-192 ct len:", (ct as ArrayBuffer).byteLength);
  console.log("aes-gcm-192 roundtrip:", Buffer.from(pt).toString());
  console.log("aes-gcm-192 generated len:", Buffer.from(generatedRaw).length);
}
await main();

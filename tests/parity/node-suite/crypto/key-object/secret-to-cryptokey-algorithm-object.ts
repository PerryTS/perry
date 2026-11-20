import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const secret = crypto.createSecretKey(Buffer.from("00112233445566778899aabbccddeeff", "hex"));
  const hmacKey = (secret as any).toCryptoKey({ name: "HMAC", hash: "SHA-384" }, true, ["sign", "verify"]);
  const sig = await crypto.subtle.sign("HMAC", hmacKey, new TextEncoder().encode("object algorithm"));
  const ok = await crypto.subtle.verify("HMAC", hmacKey, sig, new TextEncoder().encode("object algorithm"));
  console.log("toCryptoKey hmac object ok:", ok);
  console.log("toCryptoKey hmac sha384 len:", Buffer.from(sig).length);

  const aesKey = (secret as any).toCryptoKey({ name: "AES-GCM" }, true, ["encrypt", "decrypt"]);
  const iv = Buffer.from("0102030405060708090a0b0c", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, aesKey, new TextEncoder().encode("aes object"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, aesKey, ct);
  console.log("toCryptoKey aes object pt:", Buffer.from(pt).toString());
}
await main();

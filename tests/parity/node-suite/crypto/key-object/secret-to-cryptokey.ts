import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const secret = crypto.createSecretKey(Buffer.from("000102030405060708090a0b0c0d0e0f", "hex"));
  const aesKey = (secret as any).toCryptoKey("AES-GCM", true, ["encrypt", "decrypt"]);
  const iv = Buffer.from("000102030405060708090a0b", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, aesKey, new TextEncoder().encode("keyobject aes"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, aesKey, ct);
  console.log("secret toCryptoKey aes pt:", Buffer.from(pt).toString());
  const raw = await crypto.subtle.exportKey("raw", aesKey);
  console.log("secret toCryptoKey aes raw hex:", Buffer.from(raw).toString("hex"));

  const hmacSecret = crypto.createSecretKey(Buffer.from("secret-key"));
  const hmacKey = (hmacSecret as any).toCryptoKey({ name: "HMAC", hash: "SHA-256" }, true, ["sign", "verify"]);
  const sig = await crypto.subtle.sign("HMAC", hmacKey, new TextEncoder().encode("data"));
  console.log("secret toCryptoKey hmac sig len:", Buffer.from(sig).length);
  console.log("secret toCryptoKey hmac verify:", await crypto.subtle.verify("HMAC", hmacKey, sig, new TextEncoder().encode("data")));
}
await main();

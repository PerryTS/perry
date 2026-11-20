import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const ec = new TextEncoder();
  const baseKey = await crypto.subtle.importKey("raw", ec.encode("hello"), { name: "PBKDF2", hash: "SHA-256" }, false, ["deriveKey"]);
  const key = await crypto.subtle.deriveKey(
    { name: "PBKDF2", hash: "SHA-256", salt: ec.encode("there"), iterations: 5 },
    baseKey,
    { name: "HMAC", hash: "SHA-256", length: 256 },
    true,
    ["sign", "verify"],
  );
  const raw = await crypto.subtle.exportKey("raw", key);
  console.log("pbkdf2 hmac raw hex:", Buffer.from(raw).toString("hex"));
  const sig = await crypto.subtle.sign("HMAC", key, ec.encode("pbkdf2 hmac"));
  console.log("pbkdf2 hmac sig len:", Buffer.from(sig).length);
  console.log("pbkdf2 hmac verify:", await crypto.subtle.verify("HMAC", key, sig, ec.encode("pbkdf2 hmac")));
}
await main();

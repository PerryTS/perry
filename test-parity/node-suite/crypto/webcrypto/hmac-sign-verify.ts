import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const keyBytes = new TextEncoder().encode("secret-key");
  const data = new TextEncoder().encode("hello webcrypto");
  const key = await crypto.subtle.importKey("raw", keyBytes, { name: "HMAC", hash: "SHA-256" }, false, ["sign", "verify"]);
  const sig = await crypto.subtle.sign("HMAC", key, data);
  console.log("sig len:", Buffer.from(sig).length);
  console.log("sig hex:", Buffer.from(sig).toString("hex"));
  console.log("verify ok:", await crypto.subtle.verify("HMAC", key, sig, data));
  console.log("verify bad:", await crypto.subtle.verify("HMAC", key, sig, new TextEncoder().encode("bad")));
}
await main();

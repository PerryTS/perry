import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    {
      name: "RSASSA-PKCS1-v1_5",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const spki = await crypto.subtle.exportKey("spki", pair.publicKey);
  const pkcs8 = await crypto.subtle.exportKey("pkcs8", pair.privateKey);
  console.log("rsassa spki len gt0:", Buffer.from(spki).length > 0);
  console.log("rsassa pkcs8 len gt0:", Buffer.from(pkcs8).length > 0);
  const publicKey = await crypto.subtle.importKey("spki", spki, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["verify"]);
  const privateKey = await crypto.subtle.importKey("pkcs8", pkcs8, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["sign"]);
  const data = new TextEncoder().encode("imported rsassa");
  const signature = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", privateKey, data);
  console.log("rsassa import verify:", await crypto.subtle.verify("RSASSA-PKCS1-v1_5", publicKey, signature, data));
}
await main();

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
  const data = new TextEncoder().encode("webcrypto rsassa pkcs1");
  const signature = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", pair.privateKey, data);
  console.log("rsassa sig len:", Buffer.from(signature).length);
  console.log("rsassa verify ok:", await crypto.subtle.verify("RSASSA-PKCS1-v1_5", pair.publicKey, signature, data));
  console.log("rsassa verify bad:", await crypto.subtle.verify("RSASSA-PKCS1-v1_5", pair.publicKey, signature, new TextEncoder().encode("bad")));
}
await main();

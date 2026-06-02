import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  );
  const rawPublic = await crypto.subtle.exportKey("raw", pair.publicKey);
  console.log("raw public len:", Buffer.from(rawPublic).length);
  console.log("raw public prefix:", Buffer.from(rawPublic)[0]);

  const importedPublic = await crypto.subtle.importKey(
    "raw",
    rawPublic,
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["verify"],
  );
  const data = new TextEncoder().encode("raw exported public key");
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    pair.privateKey,
    data,
  );
  console.log(
    "imported verify ok:",
    await crypto.subtle.verify(
      { name: "ECDSA", hash: "SHA-256" },
      importedPublic,
      signature,
      data,
    ),
  );
}
await main();

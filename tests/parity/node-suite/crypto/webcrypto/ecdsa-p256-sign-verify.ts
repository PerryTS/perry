import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  );
  const data = new TextEncoder().encode("webcrypto ecdsa p256");
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    pair.privateKey,
    data,
  );
  console.log("ecdsa sig len:", Buffer.from(signature).length);
  console.log(
    "ecdsa verify ok:",
    await crypto.subtle.verify(
      { name: "ECDSA", hash: "SHA-256" },
      pair.publicKey,
      signature,
      data,
    ),
  );
  console.log(
    "ecdsa verify bad:",
    await crypto.subtle.verify(
      { name: "ECDSA", hash: "SHA-256" },
      pair.publicKey,
      signature,
      new TextEncoder().encode("tampered"),
    ),
  );
}
await main();

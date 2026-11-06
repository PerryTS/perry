import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    {
      name: "RSA-PSS",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const data = new TextEncoder().encode("webcrypto rsa pss");
  const algorithm = { name: "RSA-PSS", saltLength: 32 };
  const signature = await crypto.subtle.sign(algorithm, pair.privateKey, data);
  console.log("rsa-pss sig len:", Buffer.from(signature).length);
  console.log("rsa-pss verify ok:", await crypto.subtle.verify(algorithm, pair.publicKey, signature, data));
  console.log("rsa-pss verify bad:", await crypto.subtle.verify(algorithm, pair.publicKey, signature, new TextEncoder().encode("bad")));
}
await main();

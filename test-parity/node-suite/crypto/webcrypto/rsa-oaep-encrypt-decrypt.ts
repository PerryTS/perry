import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    {
      name: "RSA-OAEP",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["encrypt", "decrypt"],
  );
  const data = new TextEncoder().encode("webcrypto rsa oaep sha256");
  const ciphertext = await crypto.subtle.encrypt({ name: "RSA-OAEP" }, pair.publicKey, data);
  console.log("rsa-oaep ct len:", Buffer.from(ciphertext).length);
  const plaintext = await crypto.subtle.decrypt({ name: "RSA-OAEP" }, pair.privateKey, ciphertext);
  console.log("rsa-oaep pt:", Buffer.from(plaintext).toString());
}
await main();

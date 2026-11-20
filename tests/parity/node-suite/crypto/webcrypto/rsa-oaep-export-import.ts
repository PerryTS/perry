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
  const spki = await crypto.subtle.exportKey("spki", pair.publicKey);
  const pkcs8 = await crypto.subtle.exportKey("pkcs8", pair.privateKey);
  console.log("rsa spki len gt0:", Buffer.from(spki).length > 0);
  console.log("rsa pkcs8 len gt0:", Buffer.from(pkcs8).length > 0);
  const publicKey = await crypto.subtle.importKey("spki", spki, { name: "RSA-OAEP", hash: "SHA-256" }, true, ["encrypt"]);
  const privateKey = await crypto.subtle.importKey("pkcs8", pkcs8, { name: "RSA-OAEP", hash: "SHA-256" }, true, ["decrypt"]);
  const data = new TextEncoder().encode("rsa oaep imported");
  const ciphertext = await crypto.subtle.encrypt({ name: "RSA-OAEP" }, publicKey, data);
  const plaintext = await crypto.subtle.decrypt({ name: "RSA-OAEP" }, privateKey, ciphertext);
  console.log("rsa import roundtrip:", Buffer.from(plaintext).toString());
}
await main();

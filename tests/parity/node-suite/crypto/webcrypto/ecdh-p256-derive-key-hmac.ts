import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const alice = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveKey", "deriveBits"],
  );
  const bob = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveKey", "deriveBits"],
  );
  const aliceKey = await crypto.subtle.deriveKey(
    { name: "ECDH", public: bob.publicKey },
    alice.privateKey,
    { name: "HMAC", hash: "SHA-256", length: 256 },
    true,
    ["sign", "verify"],
  );
  const bobKey = await crypto.subtle.deriveKey(
    { name: "eCdH", public: alice.publicKey },
    bob.privateKey,
    { name: "HmAc", hash: "SHA-256", length: 256 },
    true,
    ["sign", "verify"],
  );
  const aliceRaw = await crypto.subtle.exportKey("raw", aliceKey);
  const bobRaw = await crypto.subtle.exportKey("raw", bobKey);
  console.log("deriveKey raw len:", Buffer.from(aliceRaw).length);
  console.log("deriveKey match:", Buffer.from(aliceRaw).equals(Buffer.from(bobRaw)));
  const data = new TextEncoder().encode("ecdh deriveKey hmac");
  const sig = await crypto.subtle.sign("HMAC", aliceKey, data);
  console.log("deriveKey hmac verify:", await crypto.subtle.verify("HMAC", bobKey, sig, data));
}
await main();

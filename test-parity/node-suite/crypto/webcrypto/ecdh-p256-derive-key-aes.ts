import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const alice = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveKey"],
  );
  const bob = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveKey"],
  );
  const aliceKey = await crypto.subtle.deriveKey(
    { name: "ECDH", public: bob.publicKey },
    alice.privateKey,
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const bobKey = await crypto.subtle.deriveKey(
    { name: "ECDH", public: alice.publicKey },
    bob.privateKey,
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const raw = await crypto.subtle.exportKey("raw", aliceKey);
  console.log("deriveKey aes raw len:", Buffer.from(raw).length);
  const iv = Buffer.from("000102030405060708090a0b", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, aliceKey, new TextEncoder().encode("derived aes"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, bobKey, ct);
  console.log("deriveKey aes pt:", Buffer.from(pt).toString());
}
await main();

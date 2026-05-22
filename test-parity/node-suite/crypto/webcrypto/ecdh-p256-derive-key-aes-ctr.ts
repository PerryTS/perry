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
    { name: "AES-CTR", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const bobKey = await crypto.subtle.deriveKey(
    { name: "ECDH", public: alice.publicKey },
    bob.privateKey,
    { name: "AES-CTR", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const counter = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-CTR", counter, length: 64 }, aliceKey, new TextEncoder().encode("derived ctr"));
  const pt = await crypto.subtle.decrypt({ name: "AES-CTR", counter, length: 64 }, bobKey, ct);
  console.log("deriveKey ctr pt:", Buffer.from(pt).toString());
}
await main();

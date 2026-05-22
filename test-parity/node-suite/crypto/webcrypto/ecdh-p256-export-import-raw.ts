import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const alice = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveBits"],
  );
  const bob = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveBits"],
  );
  const rawBob = await crypto.subtle.exportKey("raw", bob.publicKey);
  console.log("ecdh raw public len:", Buffer.from(rawBob).length);
  const importedBob = await crypto.subtle.importKey(
    "raw",
    rawBob,
    { name: "ECDH", namedCurve: "P-256" },
    true,
    [],
  );
  const bits = await crypto.subtle.deriveBits(
    { name: "ECDH", public: importedBob },
    alice.privateKey,
    256,
  );
  console.log("ecdh imported bits len:", Buffer.from(bits).length);
}
await main();

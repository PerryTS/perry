import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

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
const aliceBits = await crypto.subtle.deriveBits(
  { name: "ECDH", public: bob.publicKey },
  alice.privateKey,
  256,
);
const bobBits = await crypto.subtle.deriveBits(
  { name: "ECDH", public: alice.publicKey },
  bob.privateKey,
  256,
);
console.log("ecdh bits len:", Buffer.from(aliceBits).length);
console.log("ecdh match:", Buffer.from(aliceBits).equals(Buffer.from(bobBits)));
const half = await crypto.subtle.deriveBits(
  { name: "ECDH", public: bob.publicKey },
  alice.privateKey,
  128,
);
console.log("ecdh half len:", Buffer.from(half).length);

import * as crypto from "node:crypto";
import { DiffieHellman, DiffieHellmanGroup, diffieHellman, generateKeyPairSync } from "node:crypto";

console.log(
  "dh constructor export:",
  typeof crypto.DiffieHellman,
  crypto.DiffieHellman.length,
  typeof DiffieHellman,
  crypto.DiffieHellman === DiffieHellman,
);
const alice = new DiffieHellman(512);
const bob = DiffieHellman(alice.getPrime(), "buffer");
console.log("dh constructor prime equal:", alice.getPrime("hex") === bob.getPrime("hex"));
console.log("dh constructor generator equal:", alice.getGenerator("hex") === bob.getGenerator("hex"));
const alicePub = alice.generateKeys();
const bobPubHex = bob.generateKeys("hex");
const aliceSecret = alice.computeSecret(bobPubHex, "hex", "base64");
const bobSecret = bob.computeSecret(alicePub, "buffer", "base64");
console.log("dh constructor secret equal:", aliceSecret === bobSecret);

console.log(
  "dh group constructor export:",
  typeof crypto.DiffieHellmanGroup,
  crypto.DiffieHellmanGroup.length,
  typeof DiffieHellmanGroup,
  crypto.DiffieHellmanGroup === DiffieHellmanGroup,
);
const groupA = new DiffieHellmanGroup("modp5");
const groupB = DiffieHellmanGroup("modp5");
console.log("dh group constructor prime equal:", groupA.getPrime("hex") === groupB.getPrime("hex"));
console.log("dh group constructor generator equal:", groupA.getGenerator("hex") === groupB.getGenerator("hex"));
groupA.generateKeys();
groupB.generateKeys();
const groupASecret = groupA.computeSecret(groupB.getPublicKey()).toString("hex");
const groupBSecret = groupB.computeSecret(groupA.getPublicKey()).toString("hex");
console.log("dh group constructor secret equal:", groupASecret === groupBSecret);

console.log(
  "diffieHellman export:",
  typeof crypto.diffieHellman,
  crypto.diffieHellman.length,
  typeof diffieHellman,
  crypto.diffieHellman === diffieHellman,
);
const x = generateKeyPairSync("x25519");
const y = generateKeyPairSync("x25519");
const xSecret = diffieHellman({ privateKey: x.privateKey, publicKey: y.publicKey });
const ySecret = crypto.diffieHellman({ privateKey: y.privateKey, publicKey: x.publicKey });
console.log("diffieHellman named secret:", xSecret.length, xSecret.equals(ySecret));

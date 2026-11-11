import * as crypto from "node:crypto";

const alice = crypto.generateKeyPairSync("x25519");
const bob = crypto.generateKeyPairSync("x25519");

const aliceSecret = crypto.diffieHellman({
  privateKey: alice.privateKey,
  publicKey: bob.publicKey,
});
const bobSecret = crypto.diffieHellman({
  privateKey: bob.privateKey,
  publicKey: alice.publicKey,
});
const derivedAlicePublic = crypto.createPublicKey(alice.privateKey);
const bobSecretWithDerived = crypto.diffieHellman({
  privateKey: bob.privateKey,
  publicKey: derivedAlicePublic,
});

console.log("x25519 secret len:", aliceSecret.length);
console.log("x25519 secret equal:", aliceSecret.equals(bobSecret));
console.log("x25519 derived public equal:", bobSecret.equals(bobSecretWithDerived));

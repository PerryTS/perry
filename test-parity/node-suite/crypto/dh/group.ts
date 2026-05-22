import * as crypto from "node:crypto";

const alice = crypto.createDiffieHellmanGroup("modp5");
const bob = crypto.getDiffieHellman("modp5");

alice.generateKeys();
bob.generateKeys();
const aliceSecret = alice.computeSecret(bob.getPublicKey()).toString("hex");
const bobSecret = bob.computeSecret(alice.getPublicKey()).toString("hex");

console.log("dh group prime equal:", Buffer.from(alice.getPrime()).toString("hex") === Buffer.from(bob.getPrime()).toString("hex"));
console.log("dh group generator equal:", Buffer.from(alice.getGenerator()).toString("hex") === Buffer.from(bob.getGenerator()).toString("hex"));
console.log("dh group secret equal:", aliceSecret === bobSecret);

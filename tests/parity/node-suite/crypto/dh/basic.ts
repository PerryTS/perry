import * as crypto from "node:crypto";

const alice = crypto.createDiffieHellman(512);
const prime = alice.getPrime();
const bob = crypto.createDiffieHellman(prime, "buffer");

const alicePublic = alice.generateKeys();
const bobPublicHex = bob.generateKeys("hex");
const aliceSecret = alice.computeSecret(bobPublicHex, "hex", "base64");
const bobSecret = bob.computeSecret(alicePublic, "buffer", "base64");

console.log("dh public nonempty:", alicePublic.length > 0);
console.log("dh prime equal:", Buffer.from(alice.getPrime()).toString("hex") === Buffer.from(bob.getPrime()).toString("hex"));
console.log("dh secret equal:", aliceSecret === bobSecret);

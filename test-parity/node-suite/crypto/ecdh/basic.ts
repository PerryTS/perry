import * as crypto from "node:crypto";

const alice = crypto.createECDH("prime256v1");
const bob = crypto.createECDH("prime256v1");

const alicePublic = alice.generateKeys();
const bobPublic = bob.generateKeys();

const aliceSecret = alice.computeSecret(bobPublic);
const bobSecret = bob.computeSecret(alicePublic);

console.log("alice public length:", alicePublic.length);
console.log("bob public length:", bobPublic.length);
console.log("secret length:", aliceSecret.length);
console.log(
  "secret equal:",
  Buffer.from(aliceSecret).toString("hex") === Buffer.from(bobSecret).toString("hex"),
);

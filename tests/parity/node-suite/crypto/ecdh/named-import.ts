import { createECDH } from "node:crypto";

const alice = createECDH("prime256v1");
const bob = createECDH("prime256v1");

const alicePublic = alice.generateKeys();
const bobPublic = bob.generateKeys();
const aliceSecret = alice.computeSecret(bobPublic);
const bobSecret = bob.computeSecret(alicePublic);

console.log("named public length:", alicePublic.length);
console.log("named secret equal:", Buffer.from(aliceSecret).toString("hex") === Buffer.from(bobSecret).toString("hex"));

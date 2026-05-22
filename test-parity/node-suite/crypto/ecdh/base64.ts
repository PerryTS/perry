import * as crypto from "node:crypto";

const alice = crypto.createECDH("prime256v1");
const bob = crypto.createECDH("prime256v1");
const alicePublicBase64 = alice.generateKeys("base64");
const bobPublicBase64 = bob.generateKeys("base64", "compressed");
const aliceSecretBase64 = alice.computeSecret(bobPublicBase64, "base64", "base64");
const bobSecretBase64 = bob.computeSecret(alicePublicBase64, "base64", "base64");

console.log("alice base64 string:", typeof alicePublicBase64);
console.log("bob base64 string:", typeof bobPublicBase64);
console.log("secret base64 equal:", aliceSecretBase64 === bobSecretBase64);
console.log("secret raw length:", Buffer.from(aliceSecretBase64, "base64").length);

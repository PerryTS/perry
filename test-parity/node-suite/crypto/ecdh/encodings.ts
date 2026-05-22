import * as crypto from "node:crypto";

const alice = crypto.createECDH("prime256v1");
const bob = crypto.createECDH("prime256v1");

const alicePublicHex = alice.generateKeys("hex");
const bobPublicHex = bob.generateKeys("hex", "compressed");
const aliceSecretHex = alice.computeSecret(bobPublicHex, "hex", "hex");
const bobSecretHex = bob.computeSecret(alicePublicHex, "hex", "hex");

console.log("uncompressed public hex length:", alicePublicHex.length);
console.log("compressed public hex length:", bobPublicHex.length);
console.log("secret hex length:", aliceSecretHex.length);
console.log("secret hex equal:", aliceSecretHex === bobSecretHex);
const privateHexLength = alice.getPrivateKey("hex").length;
console.log("private hex valid length:", privateHexLength > 0 && privateHexLength <= 64 && privateHexLength % 2 === 0);

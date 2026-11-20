import * as crypto from "node:crypto";

const { privateKey, publicKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const plaintext = Buffer.from("rsa private encrypt public decrypt");
const encrypted = crypto.privateEncrypt(privateKey, plaintext);
const decryptedWithPublic = crypto.publicDecrypt(publicKey, encrypted);
const decryptedWithPrivate = crypto.publicDecrypt(privateKey, encrypted);

console.log("private encrypted nonempty:", encrypted.length > 0);
console.log("public decrypt text:", decryptedWithPublic.toString());
console.log("private-as-public decrypt text:", decryptedWithPrivate.toString());

import { generateKeyPairSync, privateEncrypt, publicDecrypt } from "node:crypto";

const { privateKey, publicKey } = generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const encrypted = privateEncrypt(privateKey, Buffer.from("named private encrypt"));
const decrypted = publicDecrypt(publicKey, encrypted);

console.log("named private encrypted nonempty:", encrypted.length > 0);
console.log("named public decrypt:", decrypted.toString());

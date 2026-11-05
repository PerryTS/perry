import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const pair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const data = Buffer.from("generated rsa keypair parity");
const sig = crypto.sign("RSA-SHA256", data, pair.privateKey);
const encrypted = crypto.publicEncrypt(pair.publicKey, data);

console.log("generated verify:", crypto.verify("RSA-SHA256", data, pair.publicKey, sig));
console.log("generated decrypt:", crypto.privateDecrypt(pair.privateKey, encrypted).toString());

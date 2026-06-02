import { generateKeyPairSync, sign, verify, publicEncrypt, privateDecrypt } from "node:crypto";
import { Buffer } from "node:buffer";

const { publicKey, privateKey } = generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const data = Buffer.from("named generated keypair parity");
const sig = sign("RSA-SHA384", data, privateKey);
const encrypted = publicEncrypt(publicKey, data);

console.log("named public pem:", publicKey.includes("BEGIN PUBLIC KEY"));
console.log("named private pem:", privateKey.includes("BEGIN PRIVATE KEY"));
console.log("named generated verify:", verify("RSA-SHA384", data, publicKey, sig));
console.log("named generated decrypt:", privateDecrypt(privateKey, encrypted).toString());

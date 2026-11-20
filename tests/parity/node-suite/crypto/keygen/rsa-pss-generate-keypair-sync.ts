import * as crypto from "node:crypto";

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa-pss", {
  modulusLength: 2048,
  publicExponent: 0x10001,
  hashAlgorithm: "sha256",
  mgf1HashAlgorithm: "sha256",
  saltLength: 32,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const data = Buffer.from("rsa-pss keygen parity");
const sig = crypto.sign("sha256", data, {
  key: privateKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 32,
});
const ok = crypto.verify("sha256", data, {
  key: publicKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 32,
}, sig);

console.log("rsa-pss public marker:", String(publicKey).includes("BEGIN PUBLIC KEY"));
console.log("rsa-pss private marker:", String(privateKey).includes("BEGIN PRIVATE KEY"));
console.log("rsa-pss sig length:", sig.length);
console.log("rsa-pss verify ok:", ok);

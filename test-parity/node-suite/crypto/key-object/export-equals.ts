import * as crypto from "node:crypto";

const rsa = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const ec = crypto.generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const rsaPrivA = crypto.createPrivateKey(rsa.privateKey);
const rsaPrivB = crypto.createPrivateKey(rsa.privateKey);
const rsaPubFromPriv = crypto.createPublicKey(rsa.privateKey);
const rsaPubFromPub = crypto.createPublicKey(rsa.publicKey);
const ecPrivA = crypto.createPrivateKey(ec.privateKey);
const ecPrivB = crypto.createPrivateKey(ec.privateKey);
const ecPubFromPriv = crypto.createPublicKey(ec.privateKey);
const ecPubFromPub = crypto.createPublicKey(ec.publicKey);

const rsaPubPem = (rsaPubFromPriv as any).export({ type: "spki", format: "pem" });
const rsaPrivPem = (rsaPrivA as any).export({ type: "pkcs8", format: "pem" });
const ecPubPem = (ecPubFromPriv as any).export({ type: "spki", format: "pem" });
const ecPrivPem = (ecPrivA as any).export({ type: "pkcs8", format: "pem" });

console.log("rsa public export marker:", String(rsaPubPem).includes("BEGIN PUBLIC KEY"));
console.log("rsa private export marker:", String(rsaPrivPem).includes("BEGIN PRIVATE KEY"));
console.log("ec public export marker:", String(ecPubPem).includes("BEGIN PUBLIC KEY"));
console.log("ec private export marker:", String(ecPrivPem).includes("BEGIN PRIVATE KEY"));
console.log("rsa public equals:", (rsaPubFromPriv as any).equals(rsaPubFromPub));
console.log("rsa private equals:", (rsaPrivA as any).equals(rsaPrivB));
console.log("rsa public/private equals:", (rsaPubFromPriv as any).equals(rsaPrivA));
console.log("ec public equals:", (ecPubFromPriv as any).equals(ecPubFromPub));
console.log("ec private equals:", (ecPrivA as any).equals(ecPrivB));
console.log("ec public/private equals:", (ecPubFromPriv as any).equals(ecPrivA));

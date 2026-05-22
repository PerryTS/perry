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
const ed = crypto.generateKeyPairSync("ed25519");
const x = crypto.generateKeyPairSync("x25519");

const rsaPrivate = crypto.createPrivateKey(rsa.privateKey);
const rsaPublic = crypto.createPublicKey(rsa.privateKey);
const ecPrivate = crypto.createPrivateKey(ec.privateKey);
const ecPublic = crypto.createPublicKey(ec.privateKey);

console.log("rsa private type:", (rsaPrivate as any).type);
console.log("rsa public type:", (rsaPublic as any).type);
console.log("rsa public asym:", (rsaPublic as any).asymmetricKeyType);
console.log("ec private type:", (ecPrivate as any).type);
console.log("ec private asym:", (ecPrivate as any).asymmetricKeyType);
console.log("ec public type:", (ecPublic as any).type);
console.log("ec public asym:", (ecPublic as any).asymmetricKeyType);
console.log("ec public curve:", (ecPublic as any).asymmetricKeyDetails?.namedCurve);
console.log("ed public type:", (ed.publicKey as any).type);
console.log("ed private type:", (ed.privateKey as any).type);
console.log("ed public asym:", (ed.publicKey as any).asymmetricKeyType);
console.log("ed private asym:", (ed.privateKey as any).asymmetricKeyType);
console.log("x public type:", (x.publicKey as any).type);
console.log("x private type:", (x.privateKey as any).type);
console.log("x public asym:", (x.publicKey as any).asymmetricKeyType);
console.log("x private asym:", (x.privateKey as any).asymmetricKeyType);

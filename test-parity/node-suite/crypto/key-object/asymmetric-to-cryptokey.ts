import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

function describeCryptoKey(label: string, key: CryptoKey) {
  const algorithm = key.algorithm as any;
  console.log(`${label} type:`, key.type);
  console.log(`${label} extractable:`, key.extractable);
  console.log(`${label} alg name:`, algorithm.name);
  console.log(`${label} alg detail:`, algorithm.hash?.name ?? algorithm.namedCurve ?? "none");
  console.log(`${label} usages:`, JSON.stringify(key.usages));
  console.log(`${label} instanceof:`, key instanceof CryptoKey);
}

function reportThrow(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(`${label}:`, String(value));
  } catch (error) {
    const err = error as any;
    console.log(`${label}:`, `${err.name} ${err.code ?? ""}`.trim());
  }
}

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

const rsaPrivate = crypto.createPrivateKey(rsa.privateKey);
const rsaPublic = crypto.createPublicKey(rsaPrivate);
const ecPrivate = crypto.createPrivateKey(ec.privateKey);
const ecPublic = crypto.createPublicKey(ecPrivate);
const data = new TextEncoder().encode("asymmetric KeyObject toCryptoKey");

const rsaSignKey = (rsaPrivate as any).toCryptoKey(
  { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
  true,
  ["sign"],
);
const rsaVerifyKey = (rsaPublic as any).toCryptoKey(
  { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
  true,
  ["verify"],
);
describeCryptoKey("rsa private", rsaSignKey);
describeCryptoKey("rsa public", rsaVerifyKey);
const rsaSignature = await crypto.webcrypto.subtle.sign("RSASSA-PKCS1-v1_5", rsaSignKey, data);
console.log("rsa sig len:", Buffer.from(rsaSignature).length);
console.log(
  "rsa verify ok:",
  await crypto.webcrypto.subtle.verify("RSASSA-PKCS1-v1_5", rsaVerifyKey, rsaSignature, data),
);

const rsaEncryptKey = (rsaPublic as any).toCryptoKey(
  { name: "RSA-OAEP", hash: "SHA-256" },
  false,
  ["encrypt"],
);
const rsaDecryptKey = (rsaPrivate as any).toCryptoKey(
  { name: "RSA-OAEP", hash: "SHA-256" },
  false,
  ["decrypt"],
);
describeCryptoKey("rsa oaep public", rsaEncryptKey);
const ciphertext = await crypto.webcrypto.subtle.encrypt({ name: "RSA-OAEP" }, rsaEncryptKey, data);
console.log("rsa oaep ct len:", Buffer.from(ciphertext).length);
console.log(
  "rsa oaep pt:",
  Buffer.from(await crypto.webcrypto.subtle.decrypt({ name: "RSA-OAEP" }, rsaDecryptKey, ciphertext)).toString(),
);

const ecSignKey = (ecPrivate as any).toCryptoKey(
  { name: "ECDSA", namedCurve: "P-256" },
  true,
  ["sign"],
);
const ecVerifyKey = (ecPublic as any).toCryptoKey(
  { name: "ECDSA", namedCurve: "P-256" },
  true,
  ["verify"],
);
describeCryptoKey("ec private", ecSignKey);
describeCryptoKey("ec public", ecVerifyKey);
const ecSignature = await crypto.webcrypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, ecSignKey, data);
console.log("ec sig len:", Buffer.from(ecSignature).length);
console.log(
  "ec verify ok:",
  await crypto.webcrypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, ecVerifyKey, ecSignature, data),
);

reportThrow("rsa private verify usage", () =>
  (rsaPrivate as any).toCryptoKey({ name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["verify"]),
);

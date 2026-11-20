import * as crypto from "node:crypto";

const subtle = crypto.webcrypto.subtle;
const data = new TextEncoder().encode("keyobject asymmetric toCryptoKey");

function summary(key: CryptoKey): string {
  const algorithm = key.algorithm as EcKeyAlgorithm & RsaHashedKeyAlgorithm;
  const detail = algorithm.hash
    ? `${algorithm.name}/${algorithm.hash.name}`
    : algorithm.namedCurve
      ? `${algorithm.name}/${algorithm.namedCurve}`
      : algorithm.name;
  return [
    key instanceof CryptoKey,
    key.constructor === CryptoKey,
    key.type,
    key.extractable,
    detail,
    key.usages.join(","),
  ].join("|");
}

function errorName(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}: ok`);
  } catch (err: any) {
    console.log(`${label}: ${err.name}`);
  }
}

const rsaPem = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const rsaPrivate = crypto.createPrivateKey(rsaPem.privateKey);
const rsaPublic = crypto.createPublicKey(rsaPrivate);
const rsaSign = (rsaPrivate as any).toCryptoKey(
  { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
  false,
  ["sign"],
) as CryptoKey;
const rsaVerify = (rsaPublic as any).toCryptoKey(
  { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
  true,
  ["verify"],
) as CryptoKey;
console.log("rsa sign summary:", summary(rsaSign));
console.log("rsa verify summary:", summary(rsaVerify));
const rsaSignature = await subtle.sign("RSASSA-PKCS1-v1_5", rsaSign, data);
console.log(
  "rsa verify:",
  await subtle.verify("RSASSA-PKCS1-v1_5", rsaVerify, rsaSignature, data),
);

const rsaEncrypt = (rsaPublic as any).toCryptoKey(
  { name: "RSA-OAEP", hash: "SHA-256" },
  true,
  ["encrypt", "wrapKey"],
) as CryptoKey;
const rsaDecrypt = (rsaPrivate as any).toCryptoKey(
  { name: "RSA-OAEP", hash: "SHA-256" },
  false,
  ["decrypt", "unwrapKey"],
) as CryptoKey;
console.log("rsa oaep summary:", summary(rsaEncrypt));
const ciphertext = await subtle.encrypt("RSA-OAEP", rsaEncrypt, data);
const plaintext = await subtle.decrypt("RSA-OAEP", rsaDecrypt, ciphertext);
console.log("rsa oaep plaintext:", Buffer.from(plaintext).toString());

const ecPem = crypto.generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const ecPrivate = crypto.createPrivateKey(ecPem.privateKey);
const ecPublic = crypto.createPublicKey(ecPrivate);
const ecdsaSign = (ecPrivate as any).toCryptoKey(
  { name: "ECDSA", namedCurve: "P-256" },
  false,
  ["sign"],
) as CryptoKey;
const ecdsaVerify = (ecPublic as any).toCryptoKey(
  { name: "ECDSA", namedCurve: "P-256" },
  true,
  ["verify"],
) as CryptoKey;
console.log("ecdsa sign summary:", summary(ecdsaSign));
const ecSignature = await subtle.sign({ name: "ECDSA", hash: "SHA-256" }, ecdsaSign, data);
console.log(
  "ecdsa verify:",
  await subtle.verify({ name: "ECDSA", hash: "SHA-256" }, ecdsaVerify, ecSignature, data),
);

const peer = crypto.generateKeyPairSync("ec", { namedCurve: "prime256v1" });
const ecdhPrivate = (ecPrivate as any).toCryptoKey(
  { name: "ECDH", namedCurve: "P-256" },
  false,
  ["deriveBits"],
) as CryptoKey;
const ecdhPublic = (peer.publicKey as any).toCryptoKey(
  { name: "ECDH", namedCurve: "P-256" },
  true,
  [],
) as CryptoKey;
console.log("ecdh private summary:", summary(ecdhPrivate));
const derived = await subtle.deriveBits({ name: "ECDH", public: ecdhPublic }, ecdhPrivate, 128);
console.log("ecdh bits length:", Buffer.from(derived).length);

const ed = crypto.generateKeyPairSync("ed25519");
const edSign = (ed.privateKey as any).toCryptoKey({ name: "Ed25519" }, false, ["sign"]);
const edVerify = (ed.publicKey as any).toCryptoKey("Ed25519", true, ["verify"]);
console.log("ed verify summary:", summary(edVerify));
const edSignature = await subtle.sign("Ed25519", edSign, data);
console.log("ed verify:", await subtle.verify("Ed25519", edVerify, edSignature, data));

const x = crypto.generateKeyPairSync("x25519");
const xPrivate = (x.privateKey as any).toCryptoKey({ name: "X25519" }, false, ["deriveBits"]);
const xPublic = (x.publicKey as any).toCryptoKey("X25519", true, []);
console.log("x25519 public summary:", summary(xPublic));
const xBits = await subtle.deriveBits({ name: "X25519", public: xPublic }, xPrivate, 128);
console.log("x25519 bits length:", Buffer.from(xBits).length);

errorName("rsa wrong usage", () =>
  (rsaPublic as any).toCryptoKey(
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    true,
    ["sign"],
  )
);
errorName("rsa missing usages", () =>
  (rsaPublic as any).toCryptoKey(
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    true,
  )
);
errorName("ec curve mismatch", () =>
  (ecPublic as any).toCryptoKey({ name: "ECDSA", namedCurve: "P-384" }, true, ["verify"])
);
errorName("unknown algorithm", () =>
  (rsaPublic as any).toCryptoKey("AES-GCM", true, ["encrypt"])
);

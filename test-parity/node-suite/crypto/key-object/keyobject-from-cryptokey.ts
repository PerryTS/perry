// #6302: `KeyObject.from()` must accept every CryptoKey shape Node accepts —
// asymmetric (public/private) keys and secret keys whose backing bytes did not
// come from a KeyObject export — and must throw for genuinely invalid input
// instead of silently returning `undefined`.
import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const KeyObject = (crypto as any).KeyObject;
const subtle = crypto.webcrypto.subtle;

function report(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err: any) {
    console.log(`${label}:`, "err", err.name, err.code ?? "");
  }
}

// ── secret CryptoKey imported from a plain Buffer (not a KeyObject export) ──
const hmacKey = await subtle.importKey(
  "raw",
  Buffer.from("00112233445566778899aabbccddeeff", "hex"),
  { name: "HMAC", hash: "SHA-256" },
  true,
  ["sign"],
);
const hmacKo = KeyObject.from(hmacKey);
console.log("hmac type:", hmacKo.type);
console.log("hmac instanceof KeyObject:", hmacKo instanceof KeyObject);
console.log("hmac export hex:", hmacKo.export().toString("hex"));
console.log("hmac symmetricKeySize:", hmacKo.symmetricKeySize);
console.log("hmac asymmetricKeyType:", hmacKo.asymmetricKeyType);

const aesKey = await subtle.importKey(
  "raw",
  Buffer.from("000102030405060708090a0b0c0d0e0f", "hex"),
  { name: "AES-GCM" },
  true,
  ["encrypt", "decrypt"],
);
const aesKo = KeyObject.from(aesKey);
console.log("aes type:", aesKo.type);
console.log("aes export hex:", aesKo.export().toString("hex"));

// ── asymmetric CryptoKeys ───────────────────────────────────────────────────
const ed = (await subtle.generateKey({ name: "Ed25519" }, true, [
  "sign",
  "verify",
])) as CryptoKeyPair;
const edPublic = KeyObject.from(ed.publicKey);
const edPrivate = KeyObject.from(ed.privateKey);
console.log("ed25519 public type:", edPublic.type);
console.log("ed25519 private type:", edPrivate.type);
console.log("ed25519 public asymmetricKeyType:", edPublic.asymmetricKeyType);
console.log("ed25519 private asymmetricKeyType:", edPrivate.asymmetricKeyType);
console.log("ed25519 public instanceof KeyObject:", edPublic instanceof KeyObject);
console.log("ed25519 private instanceof KeyObject:", edPrivate instanceof KeyObject);
console.log("ed25519 public symmetricKeySize:", edPublic.symmetricKeySize);

const ec = (await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, [
  "sign",
  "verify",
])) as CryptoKeyPair;
const ecPublic = KeyObject.from(ec.publicKey);
const ecPrivate = KeyObject.from(ec.privateKey);
console.log("ecdsa public type:", ecPublic.type);
console.log("ecdsa private type:", ecPrivate.type);
console.log("ecdsa public asymmetricKeyType:", ecPublic.asymmetricKeyType);
console.log("ecdsa private asymmetricKeyType:", ecPrivate.asymmetricKeyType);
console.log("ecdsa public instanceof KeyObject:", ecPublic instanceof KeyObject);

const ecPublicPem = String(ecPublic.export({ format: "pem", type: "spki" }));
const ecPrivatePem = String(ecPrivate.export({ format: "pem", type: "pkcs8" }));
console.log("ecdsa public pem marker:", ecPublicPem.includes("BEGIN PUBLIC KEY"));
console.log("ecdsa private pem marker:", ecPrivatePem.includes("BEGIN PRIVATE KEY"));

// A KeyObject produced by `from()` must be usable as key material downstream.
const message = Buffer.from("keyobject from cryptokey");
const ecSignature = crypto.sign("sha256", message, ecPrivate);
console.log("ecdsa sign/verify:", crypto.verify("sha256", message, ecPublic, ecSignature));
console.log(
  "ecdsa verify via pem:",
  crypto.verify("sha256", message, ecPublicPem, ecSignature),
);

const rsa = (await subtle.generateKey(
  {
    name: "RSASSA-PKCS1-v1_5",
    modulusLength: 2048,
    publicExponent: new Uint8Array([1, 0, 1]),
    hash: "SHA-256",
  },
  true,
  ["sign", "verify"],
)) as CryptoKeyPair;
const rsaPublic = KeyObject.from(rsa.publicKey);
const rsaPrivate = KeyObject.from(rsa.privateKey);
console.log("rsa public type:", rsaPublic.type);
console.log("rsa private type:", rsaPrivate.type);
console.log("rsa public asymmetricKeyType:", rsaPublic.asymmetricKeyType);
console.log("rsa private asymmetricKeyType:", rsaPrivate.asymmetricKeyType);
console.log(
  "rsa public pem marker:",
  String(rsaPublic.export({ format: "pem", type: "spki" })).includes("BEGIN PUBLIC KEY"),
);
const rsaSignature = crypto.sign("RSA-SHA256", message, rsaPrivate);
console.log("rsa sign len:", rsaSignature.length);
console.log("rsa sign/verify:", crypto.verify("RSA-SHA256", message, rsaPublic, rsaSignature));

// ── invalid input must throw, never resolve to `undefined` ──────────────────
report("from undefined", () => KeyObject.from(undefined));
report("from null", () => KeyObject.from(null));
report("from object", () => KeyObject.from({}));
report("from string", () => KeyObject.from("not a key"));
report("from number", () => KeyObject.from(7));
report("from buffer", () => KeyObject.from(Buffer.from("00", "hex")));
report("from KeyObject", () =>
  KeyObject.from(crypto.createSecretKey(Buffer.from("0011", "hex"))),
);

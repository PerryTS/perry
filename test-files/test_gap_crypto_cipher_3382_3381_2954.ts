import crypto from "node:crypto";
import { Buffer } from "node:buffer";

// ---- #3381: cipher update/final string encoding overloads (AES-256-CBC) ----
const keyCbc = Buffer.from(
  "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "hex",
);
const ivCbc = Buffer.from("0123456789abcdef0123456789abcdef", "hex");

const cbcCipher = crypto.createCipheriv("aes-256-cbc", keyCbc, ivCbc);
let cbcEnc = cbcCipher.update("hello", "utf8", "hex");
cbcEnc += cbcCipher.final("hex");
console.log("aes-256-cbc enc:", cbcEnc);

const cbcDecipher = crypto.createDecipheriv("aes-256-cbc", keyCbc, ivCbc);
let cbcDec = cbcDecipher.update(cbcEnc, "hex", "utf8");
cbcDec += cbcDecipher.final("utf8");
console.log("aes-256-cbc dec:", cbcDec);

// ---- #3382: AES-256-GCM with a non-96-bit (16-byte) IV ----
const keyGcm = Buffer.from(
  "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "hex",
);
const ivGcm16 = Buffer.from("0123456789abcdef0123456789abcdef", "hex");

const gcmCipher = crypto.createCipheriv("aes-256-gcm", keyGcm, ivGcm16);
gcmCipher.setAAD(Buffer.from("aad"));
let gcmEnc = gcmCipher.update("hello", "utf8", "hex");
gcmEnc += gcmCipher.final("hex");
const gcmTag = gcmCipher.getAuthTag().toString("hex");
console.log("aes-256-gcm/16 enc:", gcmEnc);
console.log("aes-256-gcm/16 tag:", gcmTag);

const gcmDecipher = crypto.createDecipheriv("aes-256-gcm", keyGcm, ivGcm16);
gcmDecipher.setAAD(Buffer.from("aad"));
gcmDecipher.setAuthTag(Buffer.from(gcmTag, "hex"));
let gcmDec = gcmDecipher.update(gcmEnc, "hex", "utf8");
gcmDec += gcmDecipher.final("utf8");
console.log("aes-256-gcm/16 dec:", gcmDec);

// ---- #3382: AES-256-GCM with an 8-byte IV ----
const ivGcm8 = Buffer.from("0123456789abcdef", "hex");
const gcm8Cipher = crypto.createCipheriv("aes-256-gcm", keyGcm, ivGcm8);
let gcm8Enc = gcm8Cipher.update("hello", "utf8", "hex");
gcm8Enc += gcm8Cipher.final("hex");
const gcm8Tag = gcm8Cipher.getAuthTag().toString("hex");
console.log("aes-256-gcm/8 enc:", gcm8Enc);
console.log("aes-256-gcm/8 tag:", gcm8Tag);

const gcm8Decipher = crypto.createDecipheriv("aes-256-gcm", keyGcm, ivGcm8);
gcm8Decipher.setAuthTag(Buffer.from(gcm8Tag, "hex"));
let gcm8Dec = gcm8Decipher.update(gcm8Enc, "hex", "utf8");
gcm8Dec += gcm8Decipher.final("utf8");
console.log("aes-256-gcm/8 dec:", gcm8Dec);

// ---- #2954: createSecretKey string encoding semantics ----
const skCases: Array<[string, string]> = [
  ["abc", "hex"],
  ["abxxcd", "hex"],
  ["aGVsbG8=", "base64"],
  ["hello", "utf8"],
  ["abc", "utf16le"],
];
for (const [s, enc] of skCases) {
  const k = crypto.createSecretKey(s, enc as crypto.BinaryToTextEncoding);
  console.log(
    "secretKey",
    JSON.stringify(s),
    enc,
    k.export().toString("hex"),
  );
}

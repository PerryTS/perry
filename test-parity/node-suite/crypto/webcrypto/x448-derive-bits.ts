import { webcrypto } from "node:crypto";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;
const hex = (bytes: ArrayBuffer | Uint8Array) => Buffer.from(bytes).toString("hex");
const b64u = (bytes: Uint8Array) => Buffer.from(bytes).toString("base64url");

const rejectShape = async (label: string, fn: () => Promise<unknown>) => {
  try {
    await fn();
    console.log(`${label}: ok`);
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code ?? 0);
  }
};

const alicePrivateBytes = Buffer.from(
  "9a8f4925d1519f5775cf46b04b5800d4ee9ee8bae8bc5565d498c28dd9c9baf574a9419744897391006382a6f127ab1d9ac2d8c0a598726b",
  "hex",
);
const alicePublicBytes = Buffer.from(
  "9b08f7cc31b7e3e67d22d5aea121074a273bd2b83de09c63faa73d2c22c5d9bbc836647241d953d40c5b12da88120d53177f80e532c41fa0",
  "hex",
);
const bobPublicBytes = Buffer.from(
  "3eb7a829b0cd20f5bcfc0b599b6feccf6da4627107bdb0d4f345b43027d8b972fc3e34fb4232a13ca706dcb57aec3dae07bdc1c67bf33609",
  "hex",
);

console.log("supports generate:", SubtleCrypto.supports("generateKey", "X448"));
console.log("supports import:", SubtleCrypto.supports("importKey", "X448"));
console.log("supports export:", SubtleCrypto.supports("exportKey", "X448"));
console.log("supports deriveBits:", SubtleCrypto.supports("deriveBits", "X448"));
console.log("supports deriveKey:", SubtleCrypto.supports("deriveKey", "X448"));

const alice = await subtle.generateKey("X448", true, ["deriveBits", "deriveKey"]);
const bob = await subtle.generateKey({ name: "X448" }, true, ["deriveBits"]);
const generatedSecret1 = await subtle.deriveBits({ name: "X448", public: alice.publicKey }, bob.privateKey, 128);
const generatedSecret2 = await subtle.deriveBits({ name: "X448", public: bob.publicKey }, alice.privateKey, 128);
console.log("generated alg:", alice.publicKey.algorithm.name, alice.privateKey.algorithm.name);
console.log("generated usages:", JSON.stringify(alice.publicKey.usages), JSON.stringify(alice.privateKey.usages));
console.log("generated secret len:", generatedSecret1.byteLength);
console.log("generated secret equal:", Buffer.from(generatedSecret1).equals(Buffer.from(generatedSecret2)));

const rawPublic = await subtle.exportKey("raw", alice.publicKey);
const publicJwk = await subtle.exportKey("jwk", alice.publicKey) as JsonWebKey;
const privateJwk = await subtle.exportKey("jwk", alice.privateKey) as JsonWebKey;
console.log("raw public len:", rawPublic.byteLength);
console.log("jwk public:", publicJwk.kty, publicJwk.crv, publicJwk.x?.length, !!publicJwk.d);
console.log("jwk private:", privateJwk.kty, privateJwk.crv, privateJwk.x?.length, privateJwk.d?.length);

const vectorPrivate = await subtle.importKey(
  "jwk",
  {
    kty: "OKP",
    crv: "X448",
    x: b64u(alicePublicBytes),
    d: b64u(alicePrivateBytes),
    ext: true,
    key_ops: ["deriveBits", "deriveKey"],
  },
  "X448",
  true,
  ["deriveBits", "deriveKey"],
);
const vectorPublic = await subtle.importKey("raw", bobPublicBytes, { name: "X448" }, true, []);
const vectorSecret = await subtle.deriveBits({ name: "X448", public: vectorPublic }, vectorPrivate, 448);
const vectorSecret128 = await subtle.deriveBits({ name: "X448", public: vectorPublic }, vectorPrivate, 128);
console.log("vector secret:", hex(vectorSecret));
console.log("vector secret128:", hex(vectorSecret128));

const importedPublicJwk = await subtle.importKey("jwk", publicJwk, "X448", true, []);
const importedPrivateJwk = await subtle.importKey("jwk", privateJwk, "X448", true, ["deriveBits"]);
const importedSelfSecret = await subtle.deriveBits({ name: "X448", public: importedPublicJwk }, importedPrivateJwk, 128);
console.log("imported self len:", importedSelfSecret.byteLength);

const derivedHmac = await subtle.deriveKey(
  { name: "X448", public: vectorPublic },
  vectorPrivate,
  { name: "HMAC", hash: "SHA-256", length: 128 },
  true,
  ["sign", "verify"],
);
const derivedRaw = await subtle.exportKey("raw", derivedHmac);
console.log("deriveKey hmac:", derivedHmac.algorithm.name, (derivedHmac.algorithm as any).length, derivedRaw.byteLength);

await rejectShape("generate empty usages", () => subtle.generateKey("X448", true, []));
await rejectShape("generate sign usage", () => subtle.generateKey("X448", true, ["sign"] as any));
await rejectShape("derive 449", () => subtle.deriveBits({ name: "X448", public: vectorPublic }, vectorPrivate, 449));
await rejectShape("derive 456", () => subtle.deriveBits({ name: "X448", public: vectorPublic }, vectorPrivate, 456));
await rejectShape("import raw bad len", () => subtle.importKey("raw", new Uint8Array(55), "X448", true, []));

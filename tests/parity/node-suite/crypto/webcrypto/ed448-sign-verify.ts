import { webcrypto } from "node:crypto";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;
const hex = (bytes: ArrayBuffer | Uint8Array) => Buffer.from(bytes).toString("hex");
const b64u = (bytes: Uint8Array) => Buffer.from(bytes).toString("base64url");

const rejectName = async (label: string, fn: () => Promise<unknown>) => {
  try {
    await fn();
    console.log(`${label}: ok`);
  } catch (error: any) {
    console.log(`${label}:`, error.name);
  }
};

const vectorPrivate = Buffer.from(
  "6c82a562cb808d10d632be89c8513ebf6c929f34ddfa8c9f63c9960ef6e348a3528c8a3fcc2f044e39a3fc5b94492f8f032e7549a20098f95b",
  "hex",
);
const vectorPublic = Buffer.from(
  "5fd7449b59b461fd2ce787ec616ad46a1da1342485a70e1f8a0ea75d80e96778edf124769b46c7061bd6783df1e50f6cd1fa1abeafe8256180",
  "hex",
);

console.log("supports generate:", SubtleCrypto.supports("generateKey", "Ed448"));
console.log("supports import:", SubtleCrypto.supports("importKey", "Ed448"));
console.log("supports export:", SubtleCrypto.supports("exportKey", "Ed448"));
console.log("supports sign:", SubtleCrypto.supports("sign", "Ed448"));
console.log("supports verify:", SubtleCrypto.supports("verify", "Ed448"));

const pair = await subtle.generateKey({ name: "Ed448" }, true, ["sign", "verify"]);
const data = new TextEncoder().encode("webcrypto ed448 parity");
const signature = await subtle.sign({ name: "Ed448" }, pair.privateKey, data);
const ok = await subtle.verify({ name: "Ed448" }, pair.publicKey, signature, data);
const bad = await subtle.verify("Ed448", pair.publicKey, signature, new TextEncoder().encode("tampered"));
console.log("generated alg:", pair.publicKey.algorithm.name, pair.privateKey.algorithm.name);
console.log("generated usages:", JSON.stringify(pair.publicKey.usages), JSON.stringify(pair.privateKey.usages));
console.log("sig len:", (signature as ArrayBuffer).byteLength);
console.log("verify ok:", ok);
console.log("verify bad:", bad);
console.log("verify short sig:", await subtle.verify("Ed448", pair.publicKey, new Uint8Array(113), data));

const rawPublic = await subtle.exportKey("raw", pair.publicKey);
const publicJwk = await subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
const privateJwk = await subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;
console.log("raw public len:", (rawPublic as ArrayBuffer).byteLength);
console.log("jwk public:", publicJwk.kty, publicJwk.crv, publicJwk.x?.length, !!publicJwk.d);
console.log("jwk private:", privateJwk.kty, privateJwk.crv, privateJwk.x?.length, privateJwk.d?.length);

const importedRawPublic = await subtle.importKey("raw", rawPublic, "Ed448", true, ["verify"]);
console.log("raw imported verify:", await subtle.verify("Ed448", importedRawPublic, signature, data));

const importedJwkPublic = await subtle.importKey("jwk", publicJwk, { name: "Ed448" }, true, ["verify"]);
const importedJwkPrivate = await subtle.importKey("jwk", privateJwk, "Ed448", true, ["sign"]);
const importedSignature = await subtle.sign("Ed448", importedJwkPrivate, data);
console.log("jwk imported verify:", await subtle.verify({ name: "Ed448" }, importedJwkPublic, importedSignature, data));

const rfcPrivate = await subtle.importKey(
  "jwk",
  {
    kty: "OKP",
    crv: "Ed448",
    x: b64u(vectorPublic),
    d: b64u(vectorPrivate),
    ext: true,
    key_ops: ["sign"],
  },
  "Ed448",
  true,
  ["sign"],
);
const rfcPublic = await subtle.importKey("raw", vectorPublic, "Ed448", true, ["verify"]);
const vectorSignature = await subtle.sign("Ed448", rfcPrivate, new Uint8Array());
console.log("vector sig:", hex(vectorSignature));
console.log("vector verify:", await subtle.verify("Ed448", rfcPublic, vectorSignature, new Uint8Array()));

await rejectName("generate empty usages", () => subtle.generateKey("Ed448", true, []));
await rejectName("generate derive usage", () => subtle.generateKey("Ed448", true, ["deriveBits"] as any));
await rejectName("import raw private usage", () => subtle.importKey("raw", rawPublic, "Ed448", true, ["sign"]));
await rejectName("import raw bad len", () => subtle.importKey("raw", new Uint8Array(56), "Ed448", true, ["verify"]));
await rejectName("sign public key", () => subtle.sign("Ed448", pair.publicKey, data));
await rejectName("verify private key", () => subtle.verify("Ed448", pair.privateKey, signature, data));

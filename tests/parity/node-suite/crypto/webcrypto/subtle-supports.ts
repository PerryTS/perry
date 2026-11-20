import { webcrypto } from "node:crypto";

// Node 25 marks `SubtleCrypto.supports` and several algorithm names as
// experimental. Keep the fixture focused on stdout API parity.
(process as any).emitWarning = () => undefined;

const descriptorShape = (desc: any) => ({
  enumerable: desc?.enumerable,
  configurable: desc?.configurable,
  writable: "writable" in desc ? desc.writable : undefined,
  value: typeof desc?.value,
});

const throwsShape = (label: string, fn: () => void) => {
  try {
    fn();
    console.log(`${label}: no throw`);
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code ?? "");
  }
};

console.log("supports typeof:", typeof SubtleCrypto.supports);
console.log("supports name:", SubtleCrypto.supports.name);
console.log("supports length:", SubtleCrypto.supports.length);
console.log(
  "supports desc:",
  JSON.stringify(descriptorShape(Object.getOwnPropertyDescriptor(SubtleCrypto, "supports"))),
);
console.log("instance supports in subtle:", "supports" in webcrypto.subtle);
console.log("instance supports typeof:", typeof (webcrypto.subtle as any).supports);

for (const [label, op, algorithm] of [
  ["digest sha256", "digest", "SHA-256"],
  ["digest object", "digest", { name: "SHA-384" }],
  ["sign hmac", "sign", "HMAC"],
  ["verify ed25519", "verify", "Ed25519"],
  ["sign ed448", "sign", "Ed448"],
  ["verify ed448", "verify", "Ed448"],
  ["generate x448", "generateKey", "X448"],
  ["generate ed448", "generateKey", "Ed448"],
  ["generate mlkem512", "generateKey", "ML-KEM-512"],
  ["generate mlkem768", "generateKey", "ML-KEM-768"],
  ["generate mlkem1024", "generateKey", "ML-KEM-1024"],
  ["generate aes object", "generateKey", { name: "AES-GCM", length: 128 }],
  ["generate ecdh p256", "generateKey", { name: "ECDH", namedCurve: "P-256" }],
  ["import aes", "importKey", "AES-GCM"],
  ["import aes ocb", "importKey", "AES-OCB"],
  ["import x448", "importKey", "X448"],
  ["import ed448", "importKey", "Ed448"],
  ["import mlkem768", "importKey", "ML-KEM-768"],
  ["export rsa", "exportKey", "RSA-OAEP"],
  ["export aes ocb", "exportKey", "AES-OCB"],
  ["export x448", "exportKey", "X448"],
  ["export ed448", "exportKey", "Ed448"],
  ["export mlkem768", "exportKey", "ML-KEM-768"],

  ["digest sha3 false", "digest", "SHA-3-256"],
  ["encrypt aes ocb false", "encrypt", "AES-OCB"],
  ["wrap aes ocb false", "wrapKey", "AES-OCB"],
  ["sign kmac false", "sign", "KMAC128"],
  ["derive x448 false", "deriveBits", "X448"],
  ["encapsulateBits mlkem", "encapsulateBits", "ML-KEM-768"],
  ["decapsulateBits mlkem", "decapsulateBits", "ML-KEM-768"],
  ["encapsulateKey mlkem false", "encapsulateKey", "ML-KEM-768"],
] as const) {
  console.log(`${label}:`, SubtleCrypto.supports(op, algorithm as any));
}

throwsShape("supports missing args", () => (SubtleCrypto.supports as any)());
throwsShape("supports one arg", () => (SubtleCrypto.supports as any)("digest"));
console.log("supports undefined args:", (SubtleCrypto.supports as any)(undefined, "SHA-256"));

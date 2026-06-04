import { webcrypto } from "node:crypto";
import { Buffer } from "node:buffer";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;
const variants = [
  ["ML-KEM-512", 768],
  ["ML-KEM-768", 1088],
  ["ML-KEM-1024", 1568],
] as const;

const sameBytes = (a: ArrayBuffer | Uint8Array, b: ArrayBuffer | Uint8Array) =>
  Buffer.from(a).equals(Buffer.from(b));

const rejectName = async (label: string, fn: () => Promise<unknown>) => {
  try {
    await fn();
    console.log(`${label}: ok`);
  } catch (error: any) {
    console.log(`${label}:`, error.name);
  }
};

for (const [algorithm, ciphertextLength] of variants) {
  console.log(`algorithm: ${algorithm}`);
  console.log("supports encapsulateBits:", SubtleCrypto.supports("encapsulateBits", algorithm));
  console.log("supports decapsulateBits:", SubtleCrypto.supports("decapsulateBits", algorithm));
  console.log("supports encapsulateKey:", SubtleCrypto.supports("encapsulateKey", algorithm));
  console.log("supports decapsulateKey:", SubtleCrypto.supports("decapsulateKey", algorithm));

  const pair = await subtle.generateKey(
    { name: algorithm },
    true,
    ["encapsulateBits", "decapsulateBits", "encapsulateKey", "decapsulateKey"],
  );

  const bits = await (subtle as any).encapsulateBits(algorithm, pair.publicKey);
  const recoveredBits = await (subtle as any).decapsulateBits(
    algorithm,
    pair.privateKey,
    bits.ciphertext,
  );
  console.log(
    "bits shape:",
    Object.keys(bits).join("|"),
    bits.sharedKey.byteLength,
    bits.ciphertext.byteLength,
    bits.ciphertext.byteLength === ciphertextLength,
    sameBytes(recoveredBits, bits.sharedKey),
  );

  const keyResult = await (subtle as any).encapsulateKey(
    algorithm,
    pair.publicKey,
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const decapsulatedKey = await (subtle as any).decapsulateKey(
    algorithm,
    pair.privateKey,
    keyResult.ciphertext,
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const encapsulatedRaw = await subtle.exportKey("raw", keyResult.sharedKey);
  const decapsulatedRaw = await subtle.exportKey("raw", decapsulatedKey);
  console.log(
    "key shape:",
    Object.keys(keyResult).join("|"),
    keyResult.ciphertext.byteLength,
    keyResult.ciphertext.byteLength === ciphertextLength,
    (encapsulatedRaw as ArrayBuffer).byteLength,
    (decapsulatedRaw as ArrayBuffer).byteLength,
    sameBytes(encapsulatedRaw, decapsulatedRaw),
  );

  const noPublicUsage = await subtle.generateKey({ name: algorithm }, true, ["decapsulateBits"]);
  const noPrivateUsage = await subtle.generateKey(
    { name: algorithm },
    true,
    ["encapsulateBits", "decapsulateKey"],
  );
  await rejectName("encapsulate no usage", () =>
    (subtle as any).encapsulateBits(algorithm, noPublicUsage.publicKey),
  );
  await rejectName("decapsulate no usage", () =>
    (subtle as any).decapsulateBits(algorithm, noPrivateUsage.privateKey, bits.ciphertext),
  );
  await rejectName("bad ciphertext", () =>
    (subtle as any).decapsulateBits(algorithm, pair.privateKey, new Uint8Array(7)),
  );
}

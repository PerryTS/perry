import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

(process as any).emitWarning = () => undefined;

const subtle = crypto.subtle;
const enc = new TextEncoder();
const iv = new Uint8Array([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
const aad = enc.encode("chacha aad");
const data = enc.encode("chacha payload");

function supports(label: string, op: string, algorithm: AlgorithmIdentifier) {
  console.log(`${label}:`, SubtleCrypto.supports(op as any, algorithm as any));
}

async function rejectName(label: string, promise: Promise<unknown>) {
  try {
    await promise;
    console.log(`${label}: no reject`);
  } catch (error: any) {
    console.log(`${label}:`, error?.name ?? "");
  }
}

supports("supports generate string", "generateKey", "ChaCha20-Poly1305");
supports("supports import string", "importKey", "ChaCha20-Poly1305");
supports("supports export string", "exportKey", "ChaCha20-Poly1305");
supports("supports encrypt string", "encrypt", "ChaCha20-Poly1305");
supports("supports encrypt iv", "encrypt", { name: "ChaCha20-Poly1305", iv } as any);
supports("supports decrypt iv", "decrypt", { name: "ChaCha20-Poly1305", iv } as any);
supports("supports encrypt tag96", "encrypt", { name: "ChaCha20-Poly1305", iv, tagLength: 96 } as any);
supports("supports encrypt iv8", "encrypt", { name: "ChaCha20-Poly1305", iv: new Uint8Array(8) } as any);

const key = await subtle.generateKey(
  { name: "ChaCha20-Poly1305", length: 256 } as any,
  true,
  ["encrypt", "decrypt"],
);
console.log("key:", key.type, key.extractable, JSON.stringify(key.algorithm), key.usages.join(","));

const ciphertext = new Uint8Array(
  await subtle.encrypt({ name: "ChaCha20-Poly1305", iv, additionalData: aad } as any, key, data),
);
console.log("ciphertext len:", ciphertext.length);

const plaintext = await subtle.decrypt(
  { name: "ChaCha20-Poly1305", iv, additionalData: aad } as any,
  key,
  ciphertext,
);
console.log("plaintext:", Buffer.from(plaintext).toString());

await rejectName(
  "wrong aad",
  subtle.decrypt({ name: "ChaCha20-Poly1305", iv, additionalData: enc.encode("bad") } as any, key, ciphertext),
);
await rejectName(
  "bad tag length",
  subtle.encrypt({ name: "ChaCha20-Poly1305", iv, tagLength: 96 } as any, key, data),
);

const wrongKey = await subtle.generateKey("ChaCha20-Poly1305" as any, true, ["decrypt"]);
await rejectName(
  "wrong key",
  subtle.decrypt({ name: "ChaCha20-Poly1305", iv, additionalData: aad } as any, wrongKey, ciphertext),
);

const limited = await subtle.generateKey("ChaCha20-Poly1305" as any, false, ["encrypt"]);
console.log("limited:", limited.type, limited.extractable, JSON.stringify(limited.algorithm), limited.usages.join(","));
await rejectName(
  "limited decrypt",
  subtle.decrypt({ name: "ChaCha20-Poly1305", iv } as any, limited, ciphertext),
);
await rejectName(
  "bad usage",
  subtle.generateKey("ChaCha20-Poly1305" as any, true, ["sign" as any]),
);
await rejectName(
  "empty usages",
  subtle.generateKey("ChaCha20-Poly1305" as any, true, []),
);

const jwk = await subtle.exportKey("jwk", key) as JsonWebKey;
console.log("jwk:", jwk.kty, jwk.alg, typeof jwk.k, Boolean(jwk.k && jwk.k.length > 0));
const imported = await subtle.importKey("jwk", jwk, "ChaCha20-Poly1305" as any, true, ["encrypt", "decrypt"]);
console.log("imported:", imported.type, imported.extractable, JSON.stringify(imported.algorithm), imported.usages.join(","));
const importedCt = await subtle.encrypt({ name: "ChaCha20-Poly1305", iv } as any, imported, data);
const importedPt = await subtle.decrypt({ name: "ChaCha20-Poly1305", iv } as any, imported, importedCt);
console.log("imported plaintext:", Buffer.from(importedPt).toString());

await rejectName("raw export", subtle.exportKey("raw", key));
await rejectName(
  "raw import",
  subtle.importKey("raw", new Uint8Array(32), "ChaCha20-Poly1305" as any, true, ["encrypt"]),
);

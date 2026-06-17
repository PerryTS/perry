import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

// Node marks `SubtleCrypto.supports` as experimental. Keep the fixture
// output focused on WebCrypto behavior.
(process as any).emitWarning = () => undefined;

const enc = new TextEncoder();

const curves = [
  { name: "P-384", hash: "SHA-384", bits: 384, rawLen: 97, sigLen: 96, secretLen: 48 },
  { name: "P-521", hash: "SHA-512", bits: 528, rawLen: 133, sigLen: 132, secretLen: 66 },
] as const;

for (const curve of curves) {
  console.log(
    `supports ${curve.name}:`,
    SubtleCrypto.supports("generateKey", { name: "ECDSA", namedCurve: curve.name }),
    SubtleCrypto.supports("generateKey", { name: "ECDH", namedCurve: curve.name }),
    SubtleCrypto.supports("importKey", { name: "ECDSA", namedCurve: curve.name }),
    SubtleCrypto.supports("importKey", { name: "ECDH", namedCurve: curve.name }),
  );

  const ecdsa = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: curve.name },
    true,
    ["sign", "verify"],
  );
  const data = enc.encode(`webcrypto ecdsa ${curve.name}`);
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: curve.hash },
    ecdsa.privateKey,
    data,
  );
  const rawPublic = await crypto.subtle.exportKey("raw", ecdsa.publicKey);
  const publicJwk = await crypto.subtle.exportKey("jwk", ecdsa.publicKey) as JsonWebKey;
  const importedRawPublic = await crypto.subtle.importKey(
    "raw",
    rawPublic,
    { name: "ECDSA", namedCurve: curve.name },
    true,
    ["verify"],
  );
  const importedJwkPublic = await crypto.subtle.importKey(
    "jwk",
    publicJwk,
    { name: "ECDSA", namedCurve: curve.name },
    true,
    ["verify"],
  );
  console.log(
    `ecdsa ${curve.name}:`,
    Buffer.from(rawPublic).length,
    Buffer.from(signature).length,
    await crypto.subtle.verify({ name: "ECDSA", hash: curve.hash }, ecdsa.publicKey, signature, data),
    await crypto.subtle.verify({ name: "ECDSA", hash: curve.hash }, ecdsa.publicKey, signature, enc.encode("tampered")),
    publicJwk.kty,
    publicJwk.crv,
    await crypto.subtle.verify({ name: "ECDSA", hash: curve.hash }, importedRawPublic, signature, data),
    await crypto.subtle.verify({ name: "ECDSA", hash: curve.hash }, importedJwkPublic, signature, data),
  );

  const alice = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: curve.name },
    true,
    ["deriveBits"],
  );
  const bob = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: curve.name },
    true,
    ["deriveBits"],
  );
  const aliceBits = await crypto.subtle.deriveBits(
    { name: "ECDH", public: bob.publicKey },
    alice.privateKey,
    curve.bits,
  );
  const bobBits = await crypto.subtle.deriveBits(
    { name: "ECDH", public: alice.publicKey },
    bob.privateKey,
    curve.bits,
  );
  const bobRawPublic = await crypto.subtle.exportKey("raw", bob.publicKey);
  const bobJwkPublic = await crypto.subtle.exportKey("jwk", bob.publicKey) as JsonWebKey;
  const importedRawPeer = await crypto.subtle.importKey(
    "raw",
    bobRawPublic,
    { name: "ECDH", namedCurve: curve.name },
    true,
    [],
  );
  const importedJwkPeer = await crypto.subtle.importKey(
    "jwk",
    bobJwkPublic,
    { name: "ECDH", namedCurve: curve.name },
    true,
    [],
  );
  const importedRawBits = await crypto.subtle.deriveBits(
    { name: "ECDH", public: importedRawPeer },
    alice.privateKey,
    curve.bits,
  );
  const importedJwkBits = await crypto.subtle.deriveBits(
    { name: "ECDH", public: importedJwkPeer },
    alice.privateKey,
    curve.bits,
  );
  console.log(
    `ecdh ${curve.name}:`,
    Buffer.from(bobRawPublic).length,
    Buffer.from(aliceBits).length,
    Buffer.from(aliceBits).equals(Buffer.from(bobBits)),
    bobJwkPublic.kty,
    bobJwkPublic.crv,
    Buffer.from(aliceBits).equals(Buffer.from(importedRawBits)),
    Buffer.from(aliceBits).equals(Buffer.from(importedJwkBits)),
  );
}

try {
  const p384 = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-384" },
    true,
    ["deriveBits"],
  );
  const p521 = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-521" },
    true,
    ["deriveBits"],
  );
  await crypto.subtle.deriveBits({ name: "ECDH", public: p521.publicKey }, p384.privateKey, 384);
  console.log("ecdh cross curve: no throw");
} catch (error: any) {
  console.log("ecdh cross curve:", error.name);
}

import { generateKeyPairSync, webcrypto } from "node:crypto";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;
const variants = ["ML-DSA-44", "ML-DSA-65", "ML-DSA-87"] as const;
const message = new TextEncoder().encode("post-quantum message");

const outcome = async (label: string, fn: () => Promise<unknown>) => {
  try {
    console.log(`${label}:`, await fn());
  } catch (error: any) {
    console.log(`${label}:`, error.name, "-", error.message);
  }
};

for (const algorithm of variants) {
  console.log(`algorithm: ${algorithm}`);
  for (const operation of ["sign", "verify", "generateKey", "importKey", "exportKey", "encapsulateBits"]) {
    console.log(`supports ${operation}:`, (SubtleCrypto as any).supports(operation, algorithm));
  }

  const pair = (await subtle.generateKey(algorithm, true, ["sign", "verify"])) as CryptoKeyPair;
  console.log(
    "generated:",
    JSON.stringify(pair.publicKey.algorithm),
    pair.publicKey.type,
    pair.publicKey.extractable,
    pair.publicKey.usages.join(","),
    pair.privateKey.type,
    pair.privateKey.extractable,
    pair.privateKey.usages.join(","),
  );

  const signature = await subtle.sign(algorithm, pair.privateKey, message);
  console.log("signature:", signature.byteLength);
  console.log("verify:", await subtle.verify(algorithm, pair.publicKey, signature, message));
  console.log(
    "verify object form:",
    await subtle.verify({ name: algorithm }, pair.publicKey, signature, message),
  );
  console.log(
    "verify other message:",
    await subtle.verify(algorithm, pair.publicKey, signature, new Uint8Array([1, 2, 3])),
  );
  const tampered = new Uint8Array(signature);
  tampered[5] ^= 1;
  console.log("verify tampered:", await subtle.verify(algorithm, pair.publicKey, tampered, message));
  console.log(
    "verify short signature:",
    await subtle.verify(algorithm, pair.publicKey, new Uint8Array(10), message),
  );

  const second = await subtle.sign(algorithm, pair.privateKey, message);
  console.log(
    "hedged:",
    !Buffer.from(second).equals(Buffer.from(signature)),
    await subtle.verify(algorithm, pair.publicKey, second, message),
  );

  const context = new Uint8Array([9, 8, 7]);
  const withContext = await subtle.sign({ name: algorithm, context } as any, pair.privateKey, message);
  console.log(
    "context:",
    await subtle.verify({ name: algorithm, context } as any, pair.publicKey, withContext, message),
    await subtle.verify(algorithm, pair.publicKey, withContext, message),
    await subtle.verify(
      { name: algorithm, context: new Uint8Array([1]) } as any,
      pair.publicKey,
      withContext,
      message,
    ),
  );
  const longContext = await subtle.sign(
    { name: algorithm, context: new Uint8Array(255) } as any,
    pair.privateKey,
    message,
  );
  console.log("context 255:", longContext.byteLength);

  await outcome("sign with public key", () => subtle.sign(algorithm, pair.publicKey, message));
  await outcome("verify with private key", () =>
    subtle.verify(algorithm, pair.privateKey, signature, message),
  );
  const other = variants[(variants.indexOf(algorithm) + 1) % variants.length];
  await outcome("sign under another parameter set", () => subtle.sign(other, pair.privateKey, message));
  const ed = (await subtle.generateKey("Ed25519", true, ["sign", "verify"])) as CryptoKeyPair;
  await outcome("sign with Ed25519 key", () => subtle.sign(algorithm, ed.privateKey, message));

  await outcome("generateKey bad usage", () => subtle.generateKey(algorithm, true, ["encrypt"]));
  await outcome("generateKey no usages", () => subtle.generateKey(algorithm, true, []));
  await outcome("generateKey verify only", () => subtle.generateKey(algorithm, true, ["verify"]));

  const signOnly = (await subtle.generateKey(algorithm, false, ["sign"])) as CryptoKeyPair;
  console.log(
    "sign only:",
    signOnly.privateKey.usages.join(","),
    signOnly.publicKey.usages.length,
    signOnly.privateKey.extractable,
    signOnly.publicKey.extractable,
  );
  await outcome("verify without usage", () =>
    subtle.verify(algorithm, signOnly.publicKey, signature, message),
  );
  await outcome("export non-extractable", () => subtle.exportKey("pkcs8", signOnly.privateKey));

  // KeyObject bridge.
  const nodePair = generateKeyPairSync(algorithm.toLowerCase() as any);
  const bridged = (nodePair.publicKey as any).toCryptoKey(algorithm, true, ["verify"]) as CryptoKey;
  console.log("toCryptoKey:", JSON.stringify(bridged.algorithm), bridged.type, bridged.usages.join(","));
}

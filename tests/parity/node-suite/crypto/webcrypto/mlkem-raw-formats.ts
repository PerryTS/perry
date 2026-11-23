import { webcrypto } from "node:crypto";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;
const variants = ["ML-KEM-512", "ML-KEM-768", "ML-KEM-1024"] as const;

const outcome = async (label: string, fn: () => Promise<unknown>) => {
  try {
    console.log(`${label}:`, await fn());
  } catch (error: any) {
    console.log(`${label}:`, error.name, "-", error.message);
  }
};

for (const algorithm of variants) {
  console.log(`algorithm: ${algorithm}`);
  const pair = (await subtle.generateKey(algorithm, true, [
    "encapsulateBits",
    "decapsulateBits",
  ])) as CryptoKeyPair;

  for (const format of ["raw", "raw-public", "raw-seed", "raw-secret", "spki", "pkcs8"] as const) {
    for (const half of ["publicKey", "privateKey"] as const) {
      await outcome(`export ${format} ${half}`, async () =>
        ((await subtle.exportKey(format as any, pair[half])) as ArrayBuffer).byteLength,
      );
    }
  }
  const privateJwk = (await subtle.exportKey("jwk", pair.privateKey)) as any;
  console.log("private jwk members:", Object.keys(privateJwk).join(","));

  const rawPublic = await subtle.exportKey("raw-public" as any, pair.publicKey);
  const rawSeed = await subtle.exportKey("raw-seed" as any, pair.privateKey);
  const publicKey = await subtle.importKey("raw-public" as any, rawPublic as any, algorithm, true, [
    "encapsulateBits",
  ]);
  const privateKey = await subtle.importKey("raw-seed" as any, rawSeed as any, algorithm, true, [
    "decapsulateBits",
  ]);
  const { sharedKey, ciphertext } = await (subtle as any).encapsulateBits(algorithm, publicKey);
  const recovered = await (subtle as any).decapsulateBits(algorithm, pair.privateKey, ciphertext);
  const recoveredImported = await (subtle as any).decapsulateBits(algorithm, privateKey, ciphertext);
  console.log(
    "raw round trip:",
    publicKey.type,
    privateKey.type,
    Buffer.from(recovered).equals(Buffer.from(sharedKey)),
    Buffer.from(recoveredImported).equals(Buffer.from(sharedKey)),
  );

  await outcome("import raw", () =>
    subtle.importKey("raw", new Uint8Array(64), algorithm, true, ["decapsulateBits"]),
  );
  await outcome("import raw-seed wrong length", () =>
    subtle.importKey("raw-seed" as any, new Uint8Array(63), algorithm, true, ["decapsulateBits"]),
  );
  await outcome("import raw-public wrong length", () =>
    subtle.importKey("raw-public" as any, new Uint8Array(10), algorithm, true, ["encapsulateBits"]),
  );
  await outcome("import raw-public private usage", () =>
    subtle.importKey("raw-public" as any, rawPublic as any, algorithm, true, ["decapsulateBits"]),
  );
  await outcome("import jwk without key_ops", async () => {
    const { key_ops: _ops, ext: _ext, ...bare } = privateJwk;
    return (await subtle.importKey("jwk", bare, algorithm, true, ["decapsulateBits"])).type;
  });
  await outcome("import jwk wrong alg", () =>
    subtle.importKey("jwk", { ...privateJwk, alg: "ML-DSA-44" }, algorithm, true, ["decapsulateBits"]),
  );
}

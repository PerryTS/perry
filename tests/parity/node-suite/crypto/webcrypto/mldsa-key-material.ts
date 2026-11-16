import { webcrypto } from "node:crypto";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;
const variants = ["ML-DSA-44", "ML-DSA-65", "ML-DSA-87"] as const;
const message = new TextEncoder().encode("key material");

const outcome = async (label: string, fn: () => Promise<unknown>) => {
  try {
    console.log(`${label}:`, await fn());
  } catch (error: any) {
    console.log(`${label}:`, error.name, "-", error.message);
  }
};
const b64Bytes = (value?: string) => (value ? Buffer.from(value, "base64url").byteLength : -1);

for (const algorithm of variants) {
  console.log(`algorithm: ${algorithm}`);
  const pair = (await subtle.generateKey(algorithm, true, ["sign", "verify"])) as CryptoKeyPair;
  const signature = await subtle.sign(algorithm, pair.privateKey, message);

  // Every export format, for both halves.
  for (const format of ["raw", "raw-public", "raw-seed", "raw-secret", "spki", "pkcs8"] as const) {
    for (const half of ["publicKey", "privateKey"] as const) {
      await outcome(`export ${format} ${half}`, async () =>
        ((await subtle.exportKey(format as any, pair[half])) as ArrayBuffer).byteLength,
      );
    }
  }

  const publicJwk = (await subtle.exportKey("jwk", pair.publicKey)) as any;
  const privateJwk = (await subtle.exportKey("jwk", pair.privateKey)) as any;
  console.log(
    "public jwk:",
    Object.keys(publicJwk).join(","),
    publicJwk.kty,
    publicJwk.alg,
    publicJwk.ext,
    publicJwk.key_ops.join(","),
    b64Bytes(publicJwk.pub),
  );
  console.log(
    "private jwk:",
    Object.keys(privateJwk).join(","),
    privateJwk.kty,
    privateJwk.alg,
    privateJwk.ext,
    privateJwk.key_ops.join(","),
    b64Bytes(privateJwk.pub),
    b64Bytes(privateJwk.priv),
    privateJwk.pub === publicJwk.pub,
  );

  // Round trip through each import format: an imported public key verifies
  // the original signature; an imported private key signs for the original
  // public key.
  for (const format of ["spki", "raw-public", "jwk"] as const) {
    const exported = await subtle.exportKey(format as any, pair.publicKey);
    const imported = await subtle.importKey(format as any, exported as any, algorithm, true, ["verify"]);
    console.log(
      `import ${format}:`,
      imported.type,
      JSON.stringify(imported.algorithm),
      imported.usages.join(","),
      await subtle.verify(algorithm, imported, signature, message),
    );
  }
  for (const format of ["pkcs8", "raw-seed", "jwk"] as const) {
    const exported = await subtle.exportKey(format as any, pair.privateKey);
    const imported = await subtle.importKey(format as any, exported as any, algorithm, true, ["sign"]);
    const resigned = await subtle.sign(algorithm, imported, message);
    console.log(
      `import ${format}:`,
      imported.type,
      imported.usages.join(","),
      await subtle.verify(algorithm, pair.publicKey, resigned, message),
    );
  }

  // The seed determines the key pair.
  const seed = new Uint8Array(32).fill(7);
  const fromSeed = await subtle.importKey("raw-seed" as any, seed, algorithm, true, ["sign"]);
  const again = await subtle.importKey("raw-seed" as any, seed, algorithm, true, ["sign"]);
  const jwkA = (await subtle.exportKey("jwk", fromSeed)) as any;
  const jwkB = (await subtle.exportKey("jwk", again)) as any;
  console.log(
    "seed deterministic:",
    jwkA.pub === jwkB.pub,
    Buffer.from(jwkA.priv, "base64url").equals(Buffer.from(seed)),
    Buffer.from((await subtle.exportKey("raw-seed" as any, fromSeed)) as ArrayBuffer).equals(Buffer.from(seed)),
  );

  const rawPublic = await subtle.exportKey("raw-public" as any, pair.publicKey);
  await outcome("import spki garbage", () =>
    subtle.importKey("spki", new Uint8Array(10), algorithm, true, ["verify"]),
  );
  await outcome("import pkcs8 garbage", () =>
    subtle.importKey("pkcs8", new Uint8Array(10), algorithm, true, ["sign"]),
  );
  await outcome("import raw-public wrong length", () =>
    subtle.importKey("raw-public" as any, new Uint8Array(10), algorithm, true, ["verify"]),
  );
  await outcome("import raw-seed wrong length", () =>
    subtle.importKey("raw-seed" as any, new Uint8Array(31), algorithm, true, ["sign"]),
  );
  await outcome("import raw", () => subtle.importKey("raw", new Uint8Array(32), algorithm, true, ["sign"]));
  await outcome("import raw-seed verify usage", () =>
    subtle.importKey("raw-seed" as any, seed, algorithm, true, ["verify"]),
  );
  await outcome("import raw-seed no usages", () =>
    subtle.importKey("raw-seed" as any, seed, algorithm, true, []),
  );
  await outcome("import raw-public sign usage", () =>
    subtle.importKey("raw-public" as any, rawPublic as any, algorithm, true, ["sign"]),
  );
  await outcome("import raw-public no usages", async () =>
    (await subtle.importKey("raw-public" as any, rawPublic as any, algorithm, true, [])).usages.length,
  );

  const other = variants[(variants.indexOf(algorithm) + 1) % variants.length];
  await outcome("import spki of another parameter set", async () =>
    subtle.importKey("spki", await subtle.exportKey("spki", pair.publicKey), other, true, ["verify"]),
  );
  await outcome("import jwk without key_ops", async () => {
    const { key_ops: _ops, ext: _ext, ...bare } = privateJwk;
    return (await subtle.importKey("jwk", bare, algorithm, true, ["sign"])).type;
  });
  await outcome("import jwk key_ops mismatch", () =>
    subtle.importKey("jwk", { ...privateJwk, key_ops: [] }, algorithm, true, ["sign"]),
  );
  await outcome("import jwk wrong alg", () =>
    subtle.importKey("jwk", { ...publicJwk, alg: other }, algorithm, true, ["verify"]),
  );
  await outcome("import jwk mismatched pub", () =>
    subtle.importKey("jwk", { ...privateJwk, pub: jwkA.pub }, algorithm, true, ["sign"]),
  );
}

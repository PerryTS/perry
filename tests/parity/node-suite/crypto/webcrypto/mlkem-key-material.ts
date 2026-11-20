import { webcrypto } from "node:crypto";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;
const variants = ["ML-KEM-512", "ML-KEM-768", "ML-KEM-1024"] as const;

const b64Bytes = (value?: string) => value ? Buffer.from(value, "base64url").byteLength : -1;
const tamper = (value: string) => {
  const bytes = Buffer.from(value, "base64url");
  bytes[0] ^= 1;
  return bytes.toString("base64url");
};

const rejectName = async (label: string, fn: () => Promise<unknown>) => {
  try {
    await fn();
    console.log(`${label}: ok`);
  } catch (error: any) {
    console.log(`${label}:`, error.name);
  }
};

const cryptoKeyGetterNames = ["algorithm", "type", "extractable", "usages"] as const;
type CryptoKeyGetterName = typeof cryptoKeyGetterNames[number];

const cryptoKeyGetter = (name: CryptoKeyGetterName) =>
  Object.getOwnPropertyDescriptor(CryptoKey.prototype, name)!.get!;

const cryptoKeyGetterBrand = (name: CryptoKeyGetterName) => {
  try {
    cryptoKeyGetter(name).call({});
    return "ok";
  } catch (error: any) {
    return error.name;
  }
};

for (const algorithm of variants) {
  console.log(`algorithm: ${algorithm}`);
  console.log("supports generate:", SubtleCrypto.supports("generateKey", algorithm));
  console.log("supports import:", SubtleCrypto.supports("importKey", algorithm));
  console.log("supports export:", SubtleCrypto.supports("exportKey", algorithm));

  const pair = await subtle.generateKey({ name: algorithm }, true, ["decapsulateBits"]);
  console.log("generated alg:", pair.publicKey.algorithm.name, pair.privateKey.algorithm.name);
  console.log("generated type:", pair.publicKey.type, pair.privateKey.type);
  console.log("generated usages:", JSON.stringify(pair.publicKey.usages), JSON.stringify(pair.privateKey.usages));
  console.log("generated extractable:", pair.publicKey.extractable, pair.privateKey.extractable);
  console.log("generated instance:", pair.publicKey instanceof CryptoKey, pair.privateKey instanceof CryptoKey);
  console.log(
    "generated ctor:",
    pair.publicKey.constructor === CryptoKey,
    pair.privateKey.constructor === CryptoKey,
  );
  const descriptorAlgorithm = cryptoKeyGetter("algorithm").call(pair.publicKey) as any;
  console.log(
    "descriptor getters:",
    descriptorAlgorithm.name,
    cryptoKeyGetter("type").call(pair.privateKey),
    cryptoKeyGetter("extractable").call(pair.publicKey),
    JSON.stringify(cryptoKeyGetter("usages").call(pair.publicKey)),
  );
  console.log(
    "descriptor brands:",
    cryptoKeyGetterNames.map(cryptoKeyGetterBrand).join(","),
  );

  const spki = await subtle.exportKey("spki", pair.publicKey);
  const pkcs8 = await subtle.exportKey("pkcs8", pair.privateKey);
  const publicJwk = await subtle.exportKey("jwk", pair.publicKey) as JsonWebKey & { pub?: string; priv?: string };
  const privateJwk = await subtle.exportKey("jwk", pair.privateKey) as JsonWebKey & { pub?: string; priv?: string };
  console.log("der lens:", (spki as ArrayBuffer).byteLength, (pkcs8 as ArrayBuffer).byteLength);
  console.log(
    "jwk public:",
    publicJwk.kty,
    publicJwk.alg,
    b64Bytes(publicJwk.pub),
    !!publicJwk.priv,
    JSON.stringify(publicJwk.key_ops),
    publicJwk.ext,
  );
  console.log(
    "jwk private:",
    privateJwk.kty,
    privateJwk.alg,
    b64Bytes(privateJwk.pub),
    b64Bytes(privateJwk.priv),
    JSON.stringify(privateJwk.key_ops),
    privateJwk.ext,
  );

  const importedSpki = await subtle.importKey("spki", spki, algorithm, true, ["encapsulateBits"]);
  const importedPkcs8 = await subtle.importKey("pkcs8", pkcs8, algorithm, true, ["decapsulateBits"]);
  console.log("imported spki:", importedSpki.algorithm.name, importedSpki.type, JSON.stringify(importedSpki.usages));
  console.log("imported pkcs8:", importedPkcs8.algorithm.name, importedPkcs8.type, JSON.stringify(importedPkcs8.usages));

  const importedJwkPublic = await subtle.importKey(
    "jwk",
    { ...publicJwk, key_ops: ["encapsulateBits"] },
    algorithm,
    true,
    ["encapsulateBits"],
  );
  const importedJwkPrivate = await subtle.importKey("jwk", privateJwk, algorithm, true, ["decapsulateBits"]);
  const roundtripPrivateJwk = await subtle.exportKey("jwk", importedJwkPrivate) as JsonWebKey & {
    pub?: string;
    priv?: string;
  };
  console.log("imported jwk public:", importedJwkPublic.type, JSON.stringify(importedJwkPublic.usages));
  console.log("imported jwk private:", importedJwkPrivate.type, JSON.stringify(importedJwkPrivate.usages));
  console.log(
    "jwk private roundtrip:",
    roundtripPrivateJwk.pub === privateJwk.pub,
    roundtripPrivateJwk.priv === privateJwk.priv,
  );

  await rejectName("export raw public", () => subtle.exportKey("raw", pair.publicKey));
  await rejectName("export raw private", () => subtle.exportKey("raw", pair.privateKey));
  await rejectName("generate encap only", () => subtle.generateKey(algorithm, true, ["encapsulateBits"]));
  await rejectName("generate bad usage", () => subtle.generateKey(algorithm, true, ["deriveBits"] as any));
  await rejectName("import public decap usage", () =>
    subtle.importKey("spki", spki, algorithm, true, ["decapsulateBits"] as any),
  );
  await rejectName("import jwk keyops mismatch", () =>
    subtle.importKey("jwk", publicJwk, algorithm, true, ["encapsulateBits"]),
  );
  await rejectName("import jwk tamper pub", () =>
    subtle.importKey(
      "jwk",
      { ...privateJwk, pub: tamper(privateJwk.pub!) },
      algorithm,
      true,
      ["decapsulateBits"],
    ),
  );
}

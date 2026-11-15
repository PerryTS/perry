import crypto from "node:crypto";

// FIPS 203 sizes: ciphertext and shared key per parameter set.
for (const name of ["ml-kem-512", "ml-kem-768", "ml-kem-1024"] as const) {
  const { publicKey, privateKey } = crypto.generateKeyPairSync(name as any);
  console.log(
    name,
    "keys:",
    publicKey.type,
    publicKey.asymmetricKeyType,
    privateKey.type,
    privateKey.asymmetricKeyType,
  );
  const { sharedKey, ciphertext } = crypto.encapsulate(publicKey);
  console.log(
    name,
    "encapsulate:",
    Buffer.isBuffer(sharedKey),
    sharedKey.length,
    Buffer.isBuffer(ciphertext),
    ciphertext.length,
  );
  const recovered = crypto.decapsulate(privateKey, ciphertext);
  console.log(name, "decapsulate:", recovered.length, recovered.equals(sharedKey));

  // Each encapsulation is fresh.
  const again = crypto.encapsulate(publicKey);
  console.log(
    name,
    "fresh:",
    !again.ciphertext.equals(ciphertext),
    !again.sharedKey.equals(sharedKey),
  );

  // Implicit rejection: a tampered ciphertext decapsulates to another key.
  const tampered = Buffer.from(ciphertext);
  tampered[0] ^= 1;
  const rejected = crypto.decapsulate(privateKey, tampered);
  console.log(name, "tampered:", rejected.length, !rejected.equals(sharedKey));

  try {
    crypto.decapsulate(privateKey, ciphertext.subarray(1));
    console.log(name, "short ciphertext accepted");
  } catch (err: any) {
    console.log(name, "short ciphertext:", err.code, err.message);
  }

  // The private key holds its public key.
  const derived = crypto.createPublicKey(privateKey);
  console.log(name, "derived public:", derived.type, derived.asymmetricKeyType);
  const viaDerived = crypto.encapsulate(derived);
  console.log(
    name,
    "derived roundtrip:",
    crypto.decapsulate(privateKey, viaDerived.ciphertext).equals(viaDerived.sharedKey),
  );

  // PEM export and re-import.
  const publicPem = publicKey.export({ format: "pem", type: "spki" }) as string;
  const privatePem = privateKey.export({ format: "pem", type: "pkcs8" }) as string;
  console.log(
    name,
    "pem:",
    publicPem.split("\n")[0],
    privatePem.split("\n")[0],
    Buffer.from(privatePem.split("\n").slice(1, -2).join(""), "base64").length,
  );
  const importedPublic = crypto.createPublicKey(publicPem);
  const importedPrivate = crypto.createPrivateKey(privatePem);
  console.log(name, "imported:", importedPublic.asymmetricKeyType, importedPrivate.asymmetricKeyType);
  const viaImported = crypto.encapsulate(importedPublic);
  console.log(
    name,
    "imported roundtrip:",
    crypto.decapsulate(importedPrivate, viaImported.ciphertext).equals(viaImported.sharedKey),
  );
}

const asyncPair = await new Promise<{ publicKey: crypto.KeyObject; privateKey: crypto.KeyObject }>(
  (resolve, reject) => {
    crypto.generateKeyPair("ml-kem-768" as any, {}, (err: Error | null, publicKey: any, privateKey: any) => {
      if (err) reject(err);
      else resolve({ publicKey, privateKey });
    });
  },
);
console.log("async keygen:", asyncPair.publicKey.asymmetricKeyType, asyncPair.privateKey.type);
const asyncKem = await new Promise<any>((resolve, reject) => {
  crypto.encapsulate(asyncPair.publicKey, (err, result) => (err ? reject(err) : resolve(result)));
});
const asyncShared = await new Promise<Buffer>((resolve, reject) => {
  crypto.decapsulate(asyncPair.privateKey, asyncKem.ciphertext, (err, shared) =>
    err ? reject(err) : resolve(shared),
  );
});
console.log("async roundtrip:", asyncKem.ciphertext.length, asyncShared.equals(asyncKem.sharedKey));

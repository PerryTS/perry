import crypto from "node:crypto";

const message = Buffer.from("post-quantum message");

// FIPS 204 signature sizes per parameter set.
for (const name of ["ml-dsa-44", "ml-dsa-65", "ml-dsa-87"] as const) {
  const { publicKey, privateKey } = crypto.generateKeyPairSync(name as any);
  console.log(
    name,
    "keys:",
    publicKey.type,
    publicKey.asymmetricKeyType,
    privateKey.type,
    privateKey.asymmetricKeyType,
  );

  const signature = crypto.sign(null, message, privateKey);
  console.log(name, "signature:", Buffer.isBuffer(signature), signature.length);
  console.log(name, "verify:", crypto.verify(null, message, publicKey, signature));
  console.log(name, "verify other message:", crypto.verify(null, Buffer.from("other"), publicKey, signature));
  const tampered = Buffer.from(signature);
  tampered[10] ^= 1;
  console.log(name, "verify tampered:", crypto.verify(null, message, publicKey, tampered));
  console.log(name, "verify truncated:", crypto.verify(null, message, publicKey, signature.subarray(1)));

  // Signing is hedged: two signatures of one message differ and both verify.
  const second = crypto.sign(null, message, privateKey);
  console.log(name, "hedged:", !second.equals(signature), crypto.verify(null, message, publicKey, second));

  // The private key verifies too (it holds the public key).
  console.log(name, "verify with private:", crypto.verify(null, message, privateKey, signature));

  // FIPS 204 context string.
  const context = Buffer.from("ctx");
  const withContext = crypto.sign(null, message, { key: privateKey, context } as any);
  console.log(
    name,
    "context:",
    crypto.verify(null, message, { key: publicKey, context } as any, withContext),
    crypto.verify(null, message, publicKey, withContext),
    crypto.verify(null, message, { key: publicKey, context: Buffer.from("other") } as any, withContext),
  );
  try {
    crypto.sign(null, message, { key: privateKey, context: Buffer.alloc(256) } as any);
    console.log(name, "long context accepted");
  } catch (err: any) {
    console.log(name, "long context:", err.name, err.message);
  }

  // A key of another pair does not verify.
  const other = crypto.generateKeyPairSync(name as any);
  console.log(name, "other key:", crypto.verify(null, message, other.publicKey, signature));

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
  console.log(
    name,
    "imported roundtrip:",
    crypto.verify(null, message, importedPublic, signature),
    crypto.verify(null, message, publicKey, crypto.sign(null, message, importedPrivate)),
  );
}

const asyncPair = await new Promise<{ publicKey: crypto.KeyObject; privateKey: crypto.KeyObject }>(
  (resolve, reject) => {
    crypto.generateKeyPair("ml-dsa-65" as any, {}, (err: Error | null, publicKey: any, privateKey: any) => {
      if (err) reject(err);
      else resolve({ publicKey, privateKey });
    });
  },
);
console.log("async keygen:", asyncPair.publicKey.asymmetricKeyType, asyncPair.privateKey.type);
const asyncSignature = await new Promise<Buffer>((resolve, reject) => {
  crypto.sign(null, message, asyncPair.privateKey, (err, sig) => (err ? reject(err) : resolve(sig)));
});
const asyncOk = await new Promise<boolean>((resolve, reject) => {
  crypto.verify(null, message, asyncPair.publicKey, asyncSignature, (err, ok) =>
    err ? reject(err) : resolve(ok),
  );
});
console.log("async sign/verify:", asyncSignature.length, asyncOk);

// An ML-DSA key is not a KEM key, and an ML-KEM key does not sign.
try {
  crypto.encapsulate(asyncPair.publicKey);
  console.log("encapsulate with ml-dsa accepted");
} catch (err: any) {
  console.log("encapsulate with ml-dsa:", err.code, err.message);
}

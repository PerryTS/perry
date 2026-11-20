import crypto from "node:crypto";
import { decapsulate, encapsulate } from "node:crypto";

const pair = crypto.generateKeyPairSync("x25519");
const kem = crypto.encapsulate(pair.publicKey);
console.log("encapsulate typeof:", typeof crypto.encapsulate, crypto.encapsulate.length);
console.log("decapsulate typeof:", typeof crypto.decapsulate, crypto.decapsulate.length);
console.log(
  "encapsulate result:",
  Buffer.isBuffer(kem.sharedKey),
  kem.sharedKey.length,
  Buffer.isBuffer(kem.ciphertext),
  kem.ciphertext.length,
);
const recovered = crypto.decapsulate(pair.privateKey, kem.ciphertext);
console.log(
  "decapsulate match:",
  Buffer.isBuffer(recovered),
  recovered.length,
  Buffer.compare(kem.sharedKey, recovered) === 0,
);

const namedKem = encapsulate(pair.publicKey);
const namedRecovered = decapsulate(pair.privateKey, namedKem.ciphertext);
console.log("named exports:", Buffer.compare(namedKem.sharedKey, namedRecovered) === 0);

const asyncKem = await new Promise<any>((resolve, reject) => {
  const ret = crypto.encapsulate(pair.publicKey, (err, result) => {
    if (err) reject(err);
    else resolve(result);
  });
  console.log("encapsulate async ret:", ret === undefined);
});
const asyncRecovered = await new Promise<Buffer>((resolve, reject) => {
  const ret = crypto.decapsulate(pair.privateKey, asyncKem.ciphertext, (err, sharedKey) => {
    if (err) reject(err);
    else resolve(sharedKey);
  });
  console.log("decapsulate async ret:", ret === undefined);
});
console.log("async match:", Buffer.compare(asyncKem.sharedKey, asyncRecovered) === 0);

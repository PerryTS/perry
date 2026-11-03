import { argon2, argon2Sync } from "node:crypto";

const params = {
  message: "p",
  nonce: "12345678",
  parallelism: 1,
  tagLength: 8,
  memory: 8,
  passes: 1,
};

console.log("argon2Sync typeof:", typeof argon2Sync);
console.log("argon2 typeof:", typeof argon2);
console.log("argon2Sync length:", argon2Sync.length);
console.log("argon2 length:", argon2.length);

for (const algorithm of ["argon2id", "argon2i", "argon2d"]) {
  const key = argon2Sync(algorithm, params);
  console.log(`${algorithm} sync buffer:`, Buffer.isBuffer(key), key.length, key.toString("hex"));
}

const bufferKey = argon2Sync("argon2id", {
  ...params,
  message: Buffer.from("p"),
  nonce: Buffer.from("12345678"),
});
console.log("argon2 buffer input:", bufferKey.toString("hex"));

const typedKey = argon2Sync("argon2id", {
  ...params,
  message: new Uint8Array([112]),
  nonce: new Uint8Array([49, 50, 51, 52, 53, 54, 55, 56]),
});
console.log("argon2 typed input:", typedKey.toString("hex"));

const asyncHex = await new Promise<string>((resolve, reject) => {
  argon2("argon2id", params, (err, key) => {
    if (err) {
      reject(err);
      return;
    }
    resolve(Buffer.from(key).toString("hex"));
  });
});
console.log("argon2 async hex:", asyncHex);

// Argon2's optional inputs are part of the hash, not ignored hints (#13923).
const pepperParams = {
  message: Buffer.from("password"),
  nonce: Buffer.from("0123456789abcdef"),
  parallelism: 1,
  tagLength: 16,
  memory: 8,
  passes: 1,
};
for (const options of [
  {},
  { secret: Buffer.from("pepper") },
  { associatedData: Buffer.from("ad") },
  { secret: Buffer.from("pepper"), associatedData: Buffer.from("ad") },
  { secret: Buffer.alloc(0), associatedData: Buffer.alloc(0) },
  { secret: undefined, associatedData: undefined },
]) {
  console.log("argon2 optional sync:", argon2Sync("argon2id", { ...pepperParams, ...options }).toString("hex"));
  const hex = await new Promise<string>((resolve, reject) => {
    argon2("argon2id", { ...pepperParams, ...options }, (err, key) => {
      if (err) { reject(err); return; }
      resolve(key.toString("hex"));
    });
  });
  console.log("argon2 optional async:", hex);
}

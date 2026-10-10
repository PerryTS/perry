import { createHash, createHmac } from "node:crypto";

const data = Buffer.alloc(4097);
for (let i = 0; i < data.length; i++) data[i] = (i * 37) & 255;
for (const algorithm of ["sha384", "sha512"]) {
  for (const chunk of [1, 127, 128, 129, 1024]) {
    const hash = createHash(algorithm);
    hash.update(data.subarray(0, 113));
    const copy = hash.copy();
    for (let at = 113; at < data.length; at += chunk) {
      const bytes = data.subarray(at, at + chunk);
      hash.update(bytes);
      copy.update(bytes);
    }
    console.log(algorithm, chunk, hash.digest("hex"), copy.digest("hex"));
  }
  for (const keyLength of [0, 1, 127, 128, 129, 257]) {
    const key = Buffer.alloc(keyLength);
    for (let i = 0; i < key.length; i++) key[i] = (i * 19) & 255;
    const mac = createHmac(algorithm, key);
    for (let at = 0; at < data.length; at += 127) mac.update(data.subarray(at, at + 127));
    console.log(algorithm, keyLength, mac.digest("hex"));
  }
}

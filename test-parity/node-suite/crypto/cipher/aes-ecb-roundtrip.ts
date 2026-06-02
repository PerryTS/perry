import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

for (const [algorithm, key] of [
  ["aes-128-ecb", Buffer.alloc(16, 1)],
  ["aes-192-ecb", Buffer.alloc(24, 2)],
  ["aes-256-ecb", Buffer.alloc(32, 3)],
] as const) {
  const plaintext = Buffer.from(`ecb plaintext ${algorithm}`);
  const cipher = crypto.createCipheriv(algorithm, key, null);
  const ciphertext = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  const decipher = crypto.createDecipheriv(algorithm, key, null);
  const decrypted = Buffer.concat([decipher.update(ciphertext), decipher.final()]);
  console.log(`${algorithm} ciphertext length:`, ciphertext.length);
  console.log(`${algorithm} roundtrip:`, decrypted.equals(plaintext));
}

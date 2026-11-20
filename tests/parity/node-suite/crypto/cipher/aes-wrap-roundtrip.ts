import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const iv = Buffer.from("A6A6A6A6A6A6A6A6", "hex");
const vectors = [
  ["id-aes128-wrap", Buffer.from("000102030405060708090A0B0C0D0E0F", "hex")],
  ["aes192-wrap", Buffer.from("000102030405060708090A0B0C0D0E0F1011121314151617", "hex")],
  ["id-aes256-wrap", Buffer.from("000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F", "hex")],
] as const;
const plaintext = Buffer.from("00112233445566778899AABBCCDDEEFF", "hex");

for (const [algorithm, key] of vectors) {
  const cipher = crypto.createCipheriv(algorithm, key, iv);
  const wrapped = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  const decipher = crypto.createDecipheriv(algorithm, key, iv);
  const unwrapped = Buffer.concat([decipher.update(wrapped), decipher.final()]);
  console.log(`${algorithm} wrapped length:`, wrapped.length);
  console.log(`${algorithm} roundtrip:`, unwrapped.equals(plaintext));
}

const vectorCipher = crypto.createCipheriv("id-aes128-wrap", vectors[0][1], iv);
const vectorWrapped = Buffer.concat([vectorCipher.update(plaintext), vectorCipher.final()]);
console.log("aes128-wrap vector:", vectorWrapped.toString("hex"));

import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.alloc(32, 7);
const iv = Buffer.alloc(12, 8);
const aad = Buffer.from("authenticated data");
const plain = Buffer.from("authenticated encryption plaintext");

for (const authTagLength of [16, 12, 8, 4]) {
  const cipher = crypto.createCipheriv("aes-256-gcm", key, iv, { authTagLength });
  cipher.setAAD(aad);
  const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()]);
  const tag = cipher.getAuthTag();
  console.log("gcm tag length:", authTagLength, tag.length);
  console.log("gcm ciphertext length:", authTagLength, ciphertext.length);
}

const cipher = crypto.createCipheriv("aes-128-gcm", Buffer.alloc(16, 9), iv, { authTagLength: 12 });
const ciphertext = Buffer.concat([cipher.update("tag option"), cipher.final()]);
const tag = cipher.getAuthTag();
console.log("gcm 128 tag length:", tag.length);
console.log("gcm 128 ciphertext length:", ciphertext.length);

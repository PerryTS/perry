import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.alloc(32, 11);
const iv = Buffer.alloc(12, 12);
const aad = Buffer.from("gcm aad");
const plain = Buffer.from("gcm truncated tag decrypt plaintext");

for (const authTagLength of [16, 15, 14, 13, 12]) {
  const cipher = crypto.createCipheriv("aes-256-gcm", key, iv, { authTagLength });
  cipher.setAAD(aad);
  const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()]);
  const tag = cipher.getAuthTag();

  const decipher = crypto.createDecipheriv("aes-256-gcm", key, iv, { authTagLength });
  decipher.setAAD(aad);
  decipher.setAuthTag(tag);
  const decrypted = Buffer.concat([decipher.update(ciphertext), decipher.final()]);

  console.log("gcm decrypt tag length:", authTagLength, tag.length);
  console.log("gcm decrypt roundtrip:", authTagLength, decrypted.equals(plain));
}

const cipher128 = crypto.createCipheriv("aes-128-gcm", Buffer.alloc(16, 13), iv, { authTagLength: 12 });
const ct128 = Buffer.concat([cipher128.update(plain), cipher128.final()]);
const tag128 = cipher128.getAuthTag();
const decipher128 = crypto.createDecipheriv("aes-128-gcm", Buffer.alloc(16, 13), iv, { authTagLength: 12 });
decipher128.setAuthTag(tag128);
const pt128 = Buffer.concat([decipher128.update(ct128), decipher128.final()]);
console.log("gcm 128 decrypt roundtrip:", pt128.equals(plain));

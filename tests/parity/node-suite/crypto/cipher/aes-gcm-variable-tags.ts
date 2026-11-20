import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

for (const len of [12, 13, 14, 15, 16]) {
  const key = Buffer.alloc(24, len);
  const iv = Buffer.alloc(12, 7);
  const aad = Buffer.from("variable-tag-aad");
  const cipher = crypto.createCipheriv("aes-192-gcm", key, iv, { authTagLength: len });
  cipher.setAAD(aad, { plaintextLength: 9 });
  const ct = Buffer.concat([cipher.update("hello gcm"), cipher.final()]);
  const tag = cipher.getAuthTag();
  const decipher = crypto.createDecipheriv("aes-192-gcm", key, iv, { authTagLength: len });
  decipher.setAAD(aad, { plaintextLength: 9 });
  decipher.setAuthTag(tag);
  const pt = Buffer.concat([decipher.update(ct), decipher.final()]);
  console.log(`tag ${len}:`, tag.length, pt.toString());
}

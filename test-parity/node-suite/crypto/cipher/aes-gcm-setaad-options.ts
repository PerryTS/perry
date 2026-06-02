import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f", "hex");
const iv = Buffer.from("1a1b1c1d1e1f202122232425", "hex");
const plaintext = Buffer.from("aad options");

for (const options of [{ encoding: undefined }, { plaintextLength: undefined }, undefined] as any[]) {
  const cipher = crypto.createCipheriv("aes-256-gcm", key, iv);
  console.log("setAAD returns this:", cipher.setAAD("metadata", options) === cipher);
  const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  const tag = cipher.getAuthTag();
  const decipher = crypto.createDecipheriv("aes-256-gcm", key, iv);
  decipher.setAAD(Buffer.from("metadata"));
  decipher.setAuthTag(tag);
  const pt = Buffer.concat([decipher.update(ct), decipher.final()]);
  console.log("setAAD option roundtrip:", pt.toString());
}

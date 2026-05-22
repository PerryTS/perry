import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f", "hex");
const iv = Buffer.from("1af38c2dc2b96ffdd8669409", "hex");
const cipher = crypto.createCipheriv("aes-256-gcm", key, iv);
const enc = Buffer.concat([cipher.update(Buffer.from("hello gcm")), cipher.final()]);
const tag = cipher.getAuthTag();
console.log("cipher hex:", enc.toString("hex"));
console.log("tag hex:", tag.toString("hex"));
const decipher = crypto.createDecipheriv("aes-256-gcm", key, iv);
decipher.setAuthTag(tag);
const dec = Buffer.concat([decipher.update(enc), decipher.final()]);
console.log("roundtrip:", dec.toString());

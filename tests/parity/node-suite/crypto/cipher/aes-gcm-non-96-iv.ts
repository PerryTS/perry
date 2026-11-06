import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "hex");
const iv = Buffer.from("0123456789abcdef0123456789abcdef", "hex");
const aad = Buffer.from("aad");
const plaintext = Buffer.from("hello");

const cipher = crypto.createCipheriv("aes-256-gcm", key, iv);
cipher.setAAD(aad);
const enc = Buffer.concat([cipher.update(plaintext), cipher.final()]);
const tag = cipher.getAuthTag();
console.log("gcm16 enc:", enc.toString("hex"));
console.log("gcm16 tag:", tag.toString("hex"));

const decipher = crypto.createDecipheriv("aes-256-gcm", key, iv);
decipher.setAAD(aad);
decipher.setAuthTag(tag);
const dec = Buffer.concat([decipher.update(enc), decipher.final()]);
console.log("gcm16 dec:", dec.toString());

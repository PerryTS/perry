import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
const iv = Buffer.from("101112131415161718191a1b", "hex");
const aad = Buffer.from("feedfacedeadbeeffeedfacedeadbeefabaddad2", "hex");
const cipher = crypto.createCipheriv("aes-128-gcm", key, iv);
cipher.setAAD(aad);
const enc = Buffer.concat([cipher.update(Buffer.from("hello aad")), cipher.final()]);
const tag = cipher.getAuthTag();
console.log("enc:", enc.toString("hex"));
console.log("tag:", tag.toString("hex"));
const decipher = crypto.createDecipheriv("aes-128-gcm", key, iv);
decipher.setAAD(aad);
decipher.setAuthTag(tag);
console.log("dec:", Buffer.concat([decipher.update(enc), decipher.final()]).toString());

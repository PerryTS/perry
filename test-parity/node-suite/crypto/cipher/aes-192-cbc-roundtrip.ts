import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("000102030405060708090a0b0c0d0e0f1011121314151617", "hex");
const iv = Buffer.from("18191a1b1c1d1e1f2021222324252627", "hex");
const cipher = crypto.createCipheriv("aes-192-cbc", key, iv);
const enc = Buffer.concat([cipher.update(Buffer.from("hello aes192 cbc")), cipher.final()]);
console.log("cipher hex:", enc.toString("hex"));
const decipher = crypto.createDecipheriv("aes-192-cbc", key, iv);
const dec = Buffer.concat([decipher.update(enc), decipher.final()]);
console.log("roundtrip:", dec.toString());

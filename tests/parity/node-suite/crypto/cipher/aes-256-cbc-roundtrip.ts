import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "hex");
const iv = Buffer.from("0123456789abcdef0123456789abcdef", "hex");
const cipher = crypto.createCipheriv("aes-256-cbc", key, iv);
const enc = Buffer.concat([cipher.update(Buffer.from("hello world")), cipher.final()]);
console.log("cipher hex:", enc.toString("hex"));
const decipher = crypto.createDecipheriv("aes-256-cbc", key, iv);
const dec = Buffer.concat([decipher.update(enc), decipher.final()]);
console.log("roundtrip:", dec.toString());

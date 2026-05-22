import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "hex");
const iv = Buffer.from("0123456789abcdef0123456789abcdef", "hex");
const cipher = crypto.createCipheriv("aes-256-cbc", key, iv);
const enc = Buffer.concat([cipher.update(Buffer.from("hello")), cipher.final()]);
console.log("enc hex:", enc.toString("hex"));
const decipher = crypto.createDecipheriv("aes-256-cbc", key, iv);
console.log("dec:", Buffer.concat([decipher.update(enc), decipher.final()]).toString());

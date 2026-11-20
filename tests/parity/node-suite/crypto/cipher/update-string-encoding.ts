import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "hex");
const iv = Buffer.from("0123456789abcdef0123456789abcdef", "hex");

const cipher = crypto.createCipheriv("aes-256-cbc", key, iv);
const enc = Buffer.concat([cipher.update(Buffer.from("hello")), cipher.final()]);
console.log("buffer enc hex:", enc.toString("hex"));
const decipher = crypto.createDecipheriv("aes-256-cbc", key, iv);
console.log("buffer dec:", Buffer.concat([decipher.update(enc), decipher.final()]).toString());

const stringCipher = crypto.createCipheriv("aes-256-cbc", key, iv);
const stringEnc = stringCipher.update("hello", "utf8", "hex") + stringCipher.final("hex");
console.log("string enc hex:", stringEnc);

const stringDecipher = crypto.createDecipheriv("aes-256-cbc", key, iv);
const stringDec = stringDecipher.update(stringEnc, "hex", "utf8") + stringDecipher.final("utf8");
console.log("string dec:", stringDec);

import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = crypto.createSecretKey(Buffer.alloc(32, 5));
const iv = Buffer.alloc(16, 6);
const plaintext = Buffer.from("secret KeyObject cipher input");

const cipher = crypto.createCipheriv("aes-256-cbc", key, iv);
const ciphertext = Buffer.concat([cipher.update(plaintext), cipher.final()]);
const decipher = crypto.createDecipheriv("aes-256-cbc", key, iv);
const decrypted = Buffer.concat([decipher.update(ciphertext), decipher.final()]);

console.log("secret key cipher length:", ciphertext.length);
console.log("secret key cipher roundtrip:", decrypted.equals(plaintext));
